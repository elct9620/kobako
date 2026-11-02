//! `Bind`: a Filesystem over a host directory, from which nothing
//! outside it is reachable.
//!
//! The runtime hands a Bind one name at a time and follows every link
//! itself, so the VFS keeps the guest inside. Each name still resolves
//! through `cap_std::fs::Dir`, or the host's kernel where it confines a
//! path itself, which refuses a `..` or a link leading out of the
//! directory whatever a caller hands over, and answers alike on every
//! host, Windows among them.

mod fid;
mod lookup;
mod walk;

use std::any::Any;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, SystemTimeSpec};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions as CapOpenOptions};

use super::rdir::Rdir;
use super::{
    DentryOperations, DirEntry, FileOperations, FileSystemType, FileType, Filesystem,
    FilesystemError, Iattr, InodeOperations, Kiocb, Lookup, Metadata, OpenOptions, RenameFlag,
    SetTime,
};
use crate::vfs::{
    generic_file_llseek, generic_read_dir, Dentry, DirContext, Identity, Inode, Whence,
};
use fid::{FidCache, OpenDirFid};

/// A host directory and everything beneath it. What a guest writes lands
/// on the host and lasts as long as the directory does.
///
/// Every entry beneath it answers through a Bind too, finding what a call is
/// about through the dentry it is handed, as Linux's 9p does. A directory is
/// opened on the host when first asked, as 9p walks to a dentry's fid only
/// when a call needs one. One a guest holds open holds a host descriptor; the
/// others a walk asked about hold one only up to a bound.
#[derive(Clone)]
pub struct Bind {
    /// The host directory the Bind was made at.
    root: Arc<Dir>,
    fids: Arc<FidCache>,
}

impl Bind {
    /// Open the host directory at `path`, the one place this Filesystem
    /// reaches outside itself. One the host lets no one read is refused, so its
    /// root answers an open to read as any directory beneath it does, on every
    /// host alike.
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let root = Dir::open_ambient_dir(path, ambient_authority())?;
        may_read_dir(&root).map_err(|_| io::Error::from(io::ErrorKind::PermissionDenied))?;
        Ok(Self {
            root: Arc::new(root),
            fids: Arc::default(),
        })
    }

    /// The host directory kept for `dentry`, if one is.
    fn find_fid(&self, dentry: &Dentry) -> Option<Arc<Dir>> {
        if dentry.parent().is_none() {
            return Some(self.root.clone());
        }
        OpenDirFid::get(dentry).or_else(|| self.fids.get(dentry))
    }

    /// The host directory `dentry` names, as 9p's `v9fs_fid_lookup` finds a
    /// fid: the one kept for it, or the names beneath the nearest directory
    /// kept, read from the dentries as `build_path_from_dentry` reads them.
    /// 9p holds the rename lock through a walk; one here compares what it found
    /// with what the dentry recorded instead, as NFS compares a file handle.
    fn fid_lookup(&self, dentry: &Dentry) -> Result<Arc<Dir>, FilesystemError> {
        if let Some(held) = self.find_fid(dentry) {
            return Ok(held);
        }
        let mut names = vec![dentry.name()];
        let mut at = dentry.parent();
        let above = loop {
            let parent = at.expect("a dentry stands beneath its root, which is always held");
            if let Some(held) = self.find_fid(&parent) {
                break held;
            }
            names.push(parent.name());
            at = parent.parent();
        };
        names.reverse();
        let recorded = dentry.inode().and_then(|inode| inode.identity());
        let stands = |dir: &Dir| {
            recorded.is_none()
                || dir
                    .dir_metadata()
                    .is_ok_and(|found| Identity::of(&to_metadata(found)) == recorded)
        };
        let dir = walk::walk(&above, &names)?;
        if !stands(&dir) {
            return Err(FilesystemError::NotFound);
        }
        let dir = Arc::new(dir);
        self.fids.put(dentry, &dir);
        Ok(dir)
    }

    /// Answer `call` with the directory above `dentry` and the name it
    /// carries as the host spells it.
    fn at<T>(
        &self,
        dentry: &Dentry,
        call: impl FnOnce(&Dir, &str) -> Result<T, FilesystemError>,
    ) -> Result<T, FilesystemError> {
        let (parent, name) = dentry
            .parent_and_name()
            .expect("an entry the runtime names stands in a directory");
        call(&*self.fid_lookup(&parent)?, to_host_name(&name)?)
    }

    /// Open the file `dentry` names and keep its descriptor on the inode, so
    /// the entry answers from it however its name changes, as 9p's
    /// `v9fs_open_fid_add` keeps an open fid. What this made is recorded
    /// first, as `->atomic_open` calls `d_instantiate`.
    fn open_file(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
        create: bool,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let file = self.at(dentry, |dir, name| open_in(dir, name, options, create))?;
        let opened = to_metadata(cap_std::fs::Metadata::from_file(&file).map_err(to_error)?);
        match dentry.inode().map(|inode| inode.identity()) {
            None => dentry.add(Some(Inode::new(
                &opened,
                Arc::new(BindRegular(self.clone())),
            ))),
            // A host path may by now hold another entry than the one judged,
            // which only what opened can tell, as NFS's open compares the file
            // handle it gets back.
            Some(Some(identity)) if Identity::of(&opened) != Some(identity) => {
                return Err(FilesystemError::NotFound)
            }
            Some(_) => {}
        }
        let file = Arc::new(file);
        let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
        let kept = inode
            .private()
            .get_or_init(|| Arc::new(OpenFids::default()))
            .clone();
        if let Ok(fids) = kept.downcast::<OpenFids>() {
            let mut fids = fids.0.lock().unwrap_or_else(PoisonError::into_inner);
            fids.retain(|(fid, _)| fid.strong_count() > 0);
            fids.push((Arc::downgrade(&file), options.write || options.truncate));
        }
        Ok(Box::new(BindFile { file }))
    }

    /// Whether the entry `dentry` recorded still stands at its name in the
    /// directory above, as a walk asks each name once the one above it stood.
    /// A host may reuse an inode number for what it made in its place, so the
    /// kind must match too, as NFS compares a file handle's attributes.
    fn stands_as_recorded(&self, dentry: &Dentry) -> Result<bool, FilesystemError> {
        let inode = dentry.inode();
        if inode
            .as_ref()
            .is_some_and(|inode| inode.identity().is_none())
        {
            return Ok(true);
        }
        let found = self.at(dentry, |dir, name| {
            dir.symlink_metadata(name).map_err(to_error)
        });
        match (found, inode) {
            (Ok(found), Some(inode)) => {
                let found = to_metadata(found);
                Ok(Identity::of(&found) == inode.identity() && found.file_type == inode.kind())
            }
            (Ok(_), None) => Ok(false),
            (Err(FilesystemError::NotFound), None) => Ok(true),
            (
                Err(
                    FilesystemError::NotFound
                    | FilesystemError::NotADirectory
                    | FilesystemError::FilesystemLoop,
                ),
                _,
            ) => Ok(false),
            (Err(err), _) => Err(err),
        }
    }
}

/// The names `dir` holds, each with the kind and the identity a listing gives.
fn entries_of(dir: &Dir) -> Result<Vec<DirEntry>, FilesystemError> {
    #[cfg(unix)]
    let dev = {
        use cap_std::fs::MetadataExt;
        dir.dir_metadata().map_err(to_error)?.dev()
    };
    dir.entries()
        .map_err(to_error)?
        .map(|entry| {
            let entry = entry.map_err(to_error)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| FilesystemError::IllegalSequence)?;
            let listed = DirEntry::new(name, to_file_type(entry.file_type().map_err(to_error)?));
            #[cfg(unix)]
            let listed = {
                use std::os::unix::fs::DirEntryExt;
                listed.with_identity(dev, entry.ino())
            };
            Ok(listed)
        })
        .collect()
}

/// The directory `name` in `dir`, a link there refused as a loop, as
/// Linux's `RESOLVE_NO_SYMLINKS` refuses one, where the host names it no
/// directory.
fn open_dir_nofollow(dir: &Dir, name: &str) -> Result<Dir, FilesystemError> {
    dir.open_dir_nofollow(name)
        .map_err(|err| match dir.symlink_metadata(name) {
            Ok(metadata) if metadata.file_type().is_symlink() => FilesystemError::FilesystemLoop,
            _ => to_error(err),
        })
}

/// `name` as `cap_std` takes it. Windows reads some names as other names or as
/// devices, so there one is refused rather than reached as an entry a Mount's
/// Policy never saw.
fn to_host_name(name: &str) -> Result<&str, FilesystemError> {
    if cfg!(windows) && is_windows_alias(name) {
        return Err(FilesystemError::InvalidInput);
    }
    Ok(name)
}

/// Whether Windows reads `name` as something else: a `\` as a separator, a `:`
/// as a stream of a file, a trailing `.` or space as nothing, a wildcard as a
/// pattern, or a device's name, whatever extension follows, as the device.
fn is_windows_alias(name: &str) -> bool {
    const DEVICES: [&str; 33] = [
        "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "CLOCK$", "COM0", "COM1", "COM2", "COM3",
        "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1",
        "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
    ];
    let reserved = |c: char| c.is_ascii_control() || "\\:*?\"<>|".contains(c);
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    name.contains(reserved)
        || name.ends_with(['.', ' '])
        || DEVICES.contains(&stem.to_uppercase().as_str())
}

/// The FilesystemError a host failure names. cap-std's own refusal of a path
/// leading out of the directory carries no OS code. A Bind hands it one name and
/// no `..`, so that refusal only says a move carried the name out of the
/// directory as it was opened, as `complete_walk` answers EXDEV, and nothing
/// stands at it now. A link loop stays unnamed until std's `FilesystemLoop` is
/// stable.
fn to_error(err: io::Error) -> FilesystemError {
    match err.kind() {
        io::ErrorKind::PermissionDenied if err.raw_os_error().is_none() => {
            FilesystemError::NotFound
        }
        io::ErrorKind::NotFound => FilesystemError::NotFound,
        io::ErrorKind::AlreadyExists => FilesystemError::AlreadyExists,
        io::ErrorKind::NotADirectory => FilesystemError::NotADirectory,
        io::ErrorKind::IsADirectory => FilesystemError::IsADirectory,
        io::ErrorKind::DirectoryNotEmpty => FilesystemError::DirectoryNotEmpty,
        io::ErrorKind::PermissionDenied => FilesystemError::PermissionDenied,
        io::ErrorKind::ReadOnlyFilesystem => FilesystemError::ReadOnlyFilesystem,
        io::ErrorKind::InvalidInput => FilesystemError::InvalidInput,
        io::ErrorKind::Unsupported => FilesystemError::Unsupported,
        io::ErrorKind::StorageFull => FilesystemError::StorageFull,
        io::ErrorKind::FileTooLarge => FilesystemError::FileTooLarge,
        io::ErrorKind::CrossesDevices => FilesystemError::CrossesDevices,
        io::ErrorKind::Deadlock => FilesystemError::Deadlock,
        io::ErrorKind::ResourceBusy => FilesystemError::ResourceBusy,
        io::ErrorKind::InvalidFilename => FilesystemError::InvalidFilename,
        io::ErrorKind::NotSeekable => FilesystemError::NotSeekable,
        io::ErrorKind::Interrupted => FilesystemError::Interrupted,
        io::ErrorKind::WouldBlock => FilesystemError::WouldBlock,
        io::ErrorKind::OutOfMemory => FilesystemError::OutOfMemory,
        _ => FilesystemError::Other(err.to_string()),
    }
}

fn to_file_type(host: cap_std::fs::FileType) -> FileType {
    if host.is_symlink() {
        FileType::Symlink
    } else if host.is_dir() {
        FileType::Directory
    } else if host.is_file() {
        FileType::File
    } else {
        to_special_file_type(host)
    }
}

/// What a host entry neither a file, a directory, nor a link is, as `st_mode`
/// names it.
#[cfg(unix)]
fn to_special_file_type(host: cap_std::fs::FileType) -> FileType {
    use cap_std::fs::FileTypeExt;
    if host.is_fifo() {
        FileType::Fifo
    } else if host.is_socket() {
        FileType::Socket
    } else if host.is_block_device() {
        FileType::BlockDevice
    } else if host.is_char_device() {
        FileType::CharDevice
    } else {
        FileType::Unknown
    }
}

/// Elsewhere a Bind names no kind.
#[cfg(not(unix))]
fn to_special_file_type(_host: cap_std::fs::FileType) -> FileType {
    FileType::Unknown
}

/// What a host entry is, with the size the host reports. The host names
/// its identity, link count, and times on unix; elsewhere a Bind reports
/// none.
fn to_metadata(metadata: cap_std::fs::Metadata) -> Metadata {
    let file_type = to_file_type(metadata.file_type());
    let len = metadata.len();
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        Metadata::new(file_type, len)
            .with_identity(metadata.dev(), metadata.ino())
            .with_nlink(metadata.nlink())
            .with_times(
                unix_time(metadata.atime(), metadata.atime_nsec()),
                unix_time(metadata.mtime(), metadata.mtime_nsec()),
                unix_time(metadata.ctime(), metadata.ctime_nsec()),
            )
    }
    #[cfg(not(unix))]
    Metadata::new(file_type, len)
}

/// The time `seconds` and `nanoseconds` after the Unix epoch, as `stat`
/// reports one; `None` before the epoch, which no guest's clock reads.
#[cfg(unix)]
fn unix_time(seconds: i64, nanoseconds: i64) -> Option<std::time::SystemTime> {
    let seconds = u64::try_from(seconds).ok()?;
    let nanoseconds = u32::try_from(nanoseconds).ok()?;
    std::time::UNIX_EPOCH.checked_add(std::time::Duration::new(seconds, nanoseconds))
}

/// `time` as the host takes it, so a time of now is the host's own.
fn to_host_time(time: Option<SetTime>) -> Option<SystemTimeSpec> {
    time.map(|time| match time {
        SetTime::Now => SystemTimeSpec::SymbolicNow,
        SetTime::At(at) => SystemTimeSpec::Absolute(cap_std::time::SystemTime::from_std(at)),
    })
}

/// The descriptors open on one file, and which were opened to write, as 9p
/// keeps open fids on `i_private`.
#[derive(Default)]
struct OpenFids(Mutex<Vec<(Weak<std::fs::File>, bool)>>);

/// A descriptor open on the file `dentry` records, one open to write where
/// `writable` asks, as 9p's `v9fs_fid_find_inode` finds one.
fn v9fs_fid_find_inode(dentry: &Dentry, writable: bool) -> Option<Arc<std::fs::File>> {
    let inode = dentry.inode()?;
    let fids = inode.private().get()?.clone().downcast::<OpenFids>().ok()?;
    let fids = fids.0.lock().unwrap_or_else(PoisonError::into_inner);
    fids.iter()
        .filter(|(_, opened_to_write)| *opened_to_write || !writable)
        .find_map(|(fid, _)| fid.upgrade())
}

/// Open `name` in `dir` as `options` asks, made first where `create` says,
/// refusing one that already stands.
fn open_in(
    dir: &Dir,
    name: &str,
    options: &OpenOptions,
    create: bool,
) -> Result<std::fs::File, FilesystemError> {
    let writes = options.write || options.truncate || create;
    let mut open = CapOpenOptions::new();
    // The host refuses an open for neither reading nor writing, and one
    // that creates or truncates without writing; the runtime decides
    // what is read and written, so such opens read or write here.
    open.read(options.read || !writes)
        .write(writes)
        .create_new(create);
    // Only a file or a directory opens: a FIFO would wait for a peer
    // and a device would act on being opened. The host could swap an
    // entry in between, so what opened is checked again before its
    // first use, the open itself never waiting for a peer, nor following
    // a link swapped in, since the runtime follows a link itself.
    open.follow(FollowSymlinks::No);
    let found = dir.symlink_metadata(name);
    if let Ok(FileType::Symlink) = found.map(|metadata| to_file_type(metadata.file_type())) {
        return Err(FilesystemError::FilesystemLoop);
    }
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        use rustix::fs::OFlags;
        open.custom_flags((OFlags::NONBLOCK | OFlags::NOCTTY).bits() as i32);
    }
    let file = dir.open_with(name, &open).map_err(to_error)?;
    match to_file_type(file.metadata().map_err(to_error)?.file_type()) {
        FileType::File => {}
        FileType::Directory => return Err(FilesystemError::IsADirectory),
        _ => return Err(FilesystemError::PermissionDenied),
    }
    #[cfg(unix)]
    {
        use rustix::fs::{fcntl_getfl, fcntl_setfl, OFlags};
        let flags = fcntl_getfl(&file)
            .map_err(io::Error::from)
            .map_err(to_error)?;
        fcntl_setfl(&file, flags - OFlags::NONBLOCK)
            .map_err(io::Error::from)
            .map_err(to_error)?;
    }
    Ok(file.into_std())
}

/// The host may move or remake what a dentry names behind the runtime's
/// back, so a dentry recording an identity is valid only while the same
/// entry stands at the path it stands for, as NFS compares the handle a
/// fresh lookup finds. One recording none has nothing to compare, so nothing
/// keeps it once its holder lets go.
impl FileSystemType for Bind {
    /// Its root reports the host directory it holds, as 9p's `fill_super` asks of
    /// the attached fid, and a host that cannot answer makes no instance.
    fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
        sb.set_default_d_op(self.clone());
        let root = to_metadata(self.root.dir_metadata().map_err(to_error)?);
        sb.d_make_root(Inode::new(&root, self));
        Ok(())
    }
}

impl DentryOperations for Bind {
    fn revalidate(
        &self,
        _dir: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Result<bool, FilesystemError> {
        self.stands_as_recorded(dentry)
    }

    fn delete(&self, dentry: &Dentry) -> bool {
        dentry
            .inode()
            .is_some_and(|inode| inode.identity().is_none())
    }
}

/// A file of a Bind, or an entry a host holds that is none of the others, as
/// `v9fs_file_inode_operations_dotl` answers for one.
struct BindRegular(Bind);

/// A link of a Bind, as `v9fs_symlink_inode_operations_dotl` answers for one.
struct BindLink(Bind);

/// What an entry standing at its name in the directory above reports.
fn stat_at(bind: &Bind, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
    bind.at(dentry, |dir, name| {
        Ok(to_metadata(dir.symlink_metadata(name).map_err(to_error)?))
    })
}

impl InodeOperations for BindRegular {
    /// A file open anywhere answers from the descriptor that holds it, even
    /// once its name is gone.
    fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        match v9fs_fid_find_inode(dentry, false) {
            Some(file) => Ok(to_metadata(
                cap_std::fs::Metadata::from_file(&file).map_err(to_error)?,
            )),
            None => stat_at(&self.0, dentry),
        }
    }

    fn setattr(&self, dentry: &Dentry, attr: &Iattr) -> Result<(), FilesystemError> {
        if let Some(len) = attr.size {
            return match v9fs_fid_find_inode(dentry, true) {
                Some(file) => file.set_len(len).map_err(to_error),
                None => Err(FilesystemError::InvalidInput),
            };
        }
        // A file open anywhere takes them through what holds it, whatever stands
        // at its name by now.
        if let Some(file) = v9fs_fid_find_inode(dentry, false) {
            let mut times = std::fs::FileTimes::new();
            if let Some(atime) = attr.atime {
                times = times.set_accessed(attr.time_of(atime));
            }
            if let Some(mtime) = attr.mtime {
                times = times.set_modified(attr.time_of(mtime));
            }
            return file.set_times(times).map_err(to_error);
        }
        let (atime, mtime) = (to_host_time(attr.atime), to_host_time(attr.mtime));
        self.0.at(dentry, |dir, name| {
            dir.set_symlink_times(name, atime, mtime).map_err(to_error)
        })
    }

    fn open(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        self.0.open_file(dentry, options, false)
    }
}

impl InodeOperations for BindLink {
    fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        stat_at(&self.0, dentry)
    }

    /// Where it points as the host holds it, even outside this directory,
    /// as `->get_link` answers, since the walk judges what a link leads to. A
    /// target that is no valid UTF-8 answers `IllegalSequence`, as a guest path
    /// is one.
    fn get_link(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
        self.0.at(dentry, |dir, name| {
            dir.read_link_contents(name)
                .map_err(to_error)?
                .into_os_string()
                .into_string()
                .map_err(|_| FilesystemError::IllegalSequence)
        })
    }
}

impl InodeOperations for Bind {
    /// A directory answers from its own descriptor, which follows it wherever
    /// it moves and keeps its inode taken while it is open.
    fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        Ok(to_metadata(
            self.fid_lookup(dentry)?.dir_metadata().map_err(to_error)?,
        ))
    }

    /// A directory takes the times through the descriptor that holds it, as it
    /// answers for itself.
    fn setattr(&self, dentry: &Dentry, attr: &Iattr) -> Result<(), FilesystemError> {
        let (atime, mtime) = (to_host_time(attr.atime), to_host_time(attr.mtime));
        self.fid_lookup(dentry)?
            .set_times(".", atime, mtime)
            .map_err(to_error)
    }

    /// What stands at the name `dentry` carries, spelled as the host spells
    /// it, as a lookup hands `d_splice_alias` the inode it found, so
    /// `revalidate` can tell a dentry the host replaced behind the
    /// runtime's back. That costs one host stat a name.
    fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
        self.at(dentry, |dir, name| {
            let stored = lookup::lookup(dir, name)?;
            if stored != name {
                return Ok(Lookup::Spelled(stored));
            }
            match dir.symlink_metadata(name).map_err(to_error) {
                Ok(found) => {
                    let found = to_metadata(found);
                    let ops: Arc<dyn InodeOperations> = match found.file_type {
                        FileType::Directory => Arc::new(self.clone()),
                        FileType::Symlink => Arc::new(BindLink(self.clone())),
                        _ => Arc::new(BindRegular(self.clone())),
                    };
                    Ok(Lookup::Found(Inode::new(&found, ops)))
                }
                Err(FilesystemError::NotFound) => Ok(Lookup::Absent),
                Err(err) => Err(err),
            }
        })
    }

    fn open(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let fid = self.fid_lookup(dentry)?;
        if options.read {
            may_read_dir(&fid)?;
        }
        OpenDirFid::set(dentry, &fid);
        Ok(Box::new(BindDir(fid, Rdir::default())))
    }

    fn atomic_open(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        self.open_file(dentry, options, true)
    }

    fn mkdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.at(dentry, |dir, name| dir.create_dir(name).map_err(to_error))
    }

    fn rmdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.at(dentry, |dir, name| dir.remove_dir(name).map_err(to_error))
    }

    fn unlink(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.at(dentry, |dir, name| dir.remove_file(name).map_err(to_error))
    }

    /// A host directory takes no lock the runtime's check of the source
    /// holds, so another process of the host's changing it in between is
    /// the host's to rule out. A link moves as itself, as `rename(2)`
    /// moves one.
    fn rename(
        &self,
        from: &Dentry,
        to_dir: &dyn InodeOperations,
        to: &Dentry,
        flag: Option<RenameFlag>,
    ) -> Result<(), FilesystemError> {
        if flag.is_some() {
            return Err(FilesystemError::InvalidInput);
        }
        let target = (to_dir as &dyn Any)
            .downcast_ref::<Self>()
            .ok_or(FilesystemError::CrossesDevices)?;
        self.at(from, |from_dir, from_name| {
            target.at(to, |to_dir, to_name| {
                from_dir
                    .rename(from_name, to_dir, to_name)
                    .map_err(to_error)
            })
        })
    }
}

/// A host directory, open on the descriptor it holds, as 9p's `v9fs_dir_readdir`
/// reads the fid in `private_data`, so it follows the directory wherever it
/// moves.
struct BindDir(Arc<Dir>, Rdir);

impl FileOperations for BindDir {
    fn llseek(
        &mut self,
        dentry: &Dentry,
        pos: u64,
        offset: i64,
        whence: Whence,
    ) -> Result<u64, FilesystemError> {
        generic_file_llseek(dentry, pos, offset, whence)
    }

    fn read_iter(&mut self, _iocb: &mut Kiocb, _buf: &mut [u8]) -> Result<usize, FilesystemError> {
        generic_read_dir()
    }

    /// What a listing began with stays on the open directory until it ends, as
    /// 9p's `fid->rdir` holds what the server sent.
    fn iterate_shared(
        &mut self,
        dentry: &Dentry,
        ctx: &mut DirContext<'_>,
    ) -> Result<(), FilesystemError> {
        let dir = self.0.clone();
        self.1.iterate(dentry, ctx, || entries_of(&dir))
    }

    /// Make the host directory's names durable. Windows syncs no directory, so
    /// there the host answers for them itself.
    fn fsync(&mut self, _datasync: bool) -> Result<(), FilesystemError> {
        sync_dir(&self.0)
    }
}

/// `dir` opened to read. cap-std holds a directory for its path alone on Linux,
/// which reads and syncs nothing, so the host judges reading it only here.
#[cfg(unix)]
fn open_to_read(dir: &Dir) -> Result<rustix::fd::OwnedFd, FilesystemError> {
    use rustix::fs::{openat, Mode, OFlags};

    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
    openat(dir, ".", flags, Mode::empty()).map_err(|err| to_error(io::Error::from(err)))
}

#[cfg(unix)]
fn sync_dir(dir: &Dir) -> Result<(), FilesystemError> {
    rustix::fs::fsync(open_to_read(dir)?).map_err(|err| to_error(io::Error::from(err)))
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Dir) -> Result<(), FilesystemError> {
    Ok(())
}

/// Refuse to open `dir` to read unless the host lets it be read, as `may_open`
/// asks `inode_permission`. A file holds it for what it was opened for, so a
/// directory reached only for its path answers no such question.
#[cfg(unix)]
fn may_read_dir(dir: &Dir) -> Result<(), FilesystemError> {
    open_to_read(dir).map(drop)
}

#[cfg(not(unix))]
fn may_read_dir(_dir: &Dir) -> Result<(), FilesystemError> {
    Ok(())
}

/// An open host file. The runtime names every offset, so each access seeks
/// first.
struct BindFile {
    file: Arc<std::fs::File>,
}

impl BindFile {
    /// Windows has no append to switch one handle to for a single write,
    /// so an append that could land over another writer's is refused.
    #[cfg(not(unix))]
    fn append(&mut self, _iocb: &mut Kiocb, _buf: &[u8]) -> Result<usize, FilesystemError> {
        Err(FilesystemError::Unsupported)
    }

    /// The host places the write at the file's end as `O_APPEND` does, so
    /// another writer's append lands before or after it, never over it.
    #[cfg(unix)]
    fn append(&mut self, iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
        use rustix::fs::{fcntl_getfl, fcntl_setfl, OFlags};

        let mut file = &*self.file;
        let flags = fcntl_getfl(file)
            .map_err(io::Error::from)
            .map_err(to_error)?;
        fcntl_setfl(file, flags | OFlags::APPEND)
            .map_err(io::Error::from)
            .map_err(to_error)?;
        let written = file.write(buf);
        let end = file.stream_position();
        fcntl_setfl(file, flags)
            .map_err(io::Error::from)
            .map_err(to_error)?;
        let written = written.map_err(to_error)?;
        iocb.pos = end.map_err(to_error)?;
        Ok(written)
    }
}

impl FileOperations for BindFile {
    fn llseek(
        &mut self,
        dentry: &Dentry,
        pos: u64,
        offset: i64,
        whence: Whence,
    ) -> Result<u64, FilesystemError> {
        generic_file_llseek(dentry, pos, offset, whence)
    }

    fn read_iter(&mut self, iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        let mut file = &*self.file;
        file.seek(SeekFrom::Start(iocb.pos)).map_err(to_error)?;
        let read = file.read(buf).map_err(to_error)?;
        iocb.pos += read as u64;
        Ok(read)
    }

    fn write_iter(&mut self, iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
        let written = match iocb.append {
            true => self.append(iocb, buf)?,
            false => {
                let mut file = &*self.file;
                file.seek(SeekFrom::Start(iocb.pos)).map_err(to_error)?;
                let written = file.write(buf).map_err(to_error)?;
                iocb.pos += written as u64;
                written
            }
        };
        crate::vfs::generic_write_sync(self, iocb, written)
    }

    fn fsync(&mut self, datasync: bool) -> Result<(), FilesystemError> {
        match datasync {
            true => self.file.sync_data(),
            false => self.file.sync_all(),
        }
        .map_err(to_error)
    }

    /// Reserve the room on the host, as `v9fs_file_fallocate` asks its server. Only
    /// Linux grows a file in one step without shortening one a peer just grew, so
    /// elsewhere this refuses, as an append that cannot land in one step does.
    #[cfg(target_os = "linux")]
    fn fallocate(&mut self, offset: u64, len: u64) -> Result<(), FilesystemError> {
        let flags = rustix::fs::FallocateFlags::empty();
        rustix::fs::fallocate(&*self.file, flags, offset, len)
            .map_err(|err| to_error(io::Error::from(err)))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::vfs::kern_path::{kern_path, KernPath};

    /// A Bind reports times only where the host names them.
    #[cfg(unix)]
    mod set_times {
        use super::*;
        use crate::filesystem::{Overlay, Tmpfs};
        use crate::mount::{Access, Mount};
        use crate::vfs::{
            do_file_open, do_utimes, do_utimes_fd, vfs_fstatat, OpenHow, Path as VfsPath,
        };

        /// One second past the Unix epoch, which no clock is at.
        fn moment() -> std::time::SystemTime {
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)
        }

        fn set_modified() -> Iattr {
            Iattr::times(None, Some(SetTime::At(moment())))
        }

        /// The root of `filesystem` mounted to change.
        fn root_of(filesystem: Arc<dyn FileSystemType>) -> VfsPath {
            let mount = Mount::new(
                "/data",
                Filesystem::new(filesystem).expect("the instance must stand"),
                Access::ReadWrite,
            )
            .expect("/data is a guest path");
            VfsPath::root(mount.mnt())
        }

        /// An Overlay of `lower` beneath an empty upper.
        fn overlay_of(lower: Bind) -> Arc<Overlay> {
            Arc::new(
                Overlay::new(
                    vec![Filesystem::new(Arc::new(lower)).expect("the instance must stand")],
                    Some(Filesystem::new(Arc::new(Tmpfs::new())).expect("the instance must stand")),
                )
                .unwrap_or_else(|_| panic!("the Overlay must stack")),
            )
        }

        /// What a stat of `path` beneath `root` reports as its modified time, once
        /// it was set.
        fn modified_after_setting(root: &VfsPath, path: &str) -> Option<std::time::SystemTime> {
            do_utimes(&Arc::default(), root, path, false, &set_modified())
                .expect("the time must be set");
            vfs_fstatat(&Arc::default(), root, path, false)
                .ok()
                .and_then(|metadata| metadata.modified)
        }

        /// When the host last changed what `path` holds.
        fn host_modified(path: &std::path::Path) -> Option<std::time::SystemTime> {
            fs::metadata(path).and_then(|found| found.modified()).ok()
        }

        /// A host directory holding `dir/file.txt`.
        fn host_with_a_file(name: &str) -> HostDir {
            let host = HostDir::new(name);
            fs::create_dir(host.0.join("dir")).expect("the host directory must be made");
            fs::write(host.0.join("dir/file.txt"), "x").expect("the host file must be written");
            host
        }

        fn bind_of(host: &HostDir) -> Bind {
            Bind::new(&host.0).expect("the host directory must open")
        }

        // @behavior FS-185
        #[test]
        fn a_built_in_filesystem_keeps_the_times_a_caller_sets() {
            let host = host_with_a_file("set-times");
            let tmpfs: Tmpfs = [("dir/file.txt", "x")].into_iter().collect();
            let roots = [
                root_of(Arc::new(tmpfs)),
                root_of(Arc::new(bind_of(&host))),
                root_of(overlay_of(bind_of(&host))),
            ];
            let answers = roots.map(|root| {
                (
                    modified_after_setting(&root, "dir/file.txt"),
                    modified_after_setting(&root, "dir"),
                )
            });
            let set = Some(moment());
            assert_eq!(
                (answers, host_modified(&host.0.join("dir/file.txt"))),
                ([(set, set); 3], set),
                "vfs::do_utimes on a file and a directory of a Tmpfs, a Bind, and an Overlay must \
             keep the time, a Bind's on the host"
            );
        }

        // @behavior FS-186
        #[test]
        fn a_directory_time_set_through_an_overlay_leaves_its_lower_layer() {
            let host = host_with_a_file("overlay-dir-time");
            let before = host_modified(&host.0.join("dir"));
            let seen = modified_after_setting(&root_of(overlay_of(bind_of(&host))), "dir");
            assert_eq!(
            (seen, host_modified(&host.0.join("dir")) == before),
            (Some(moment()), true),
            "vfs::do_utimes on a directory through an Overlay must show the time and leave the \
             lower Layer's directory as it was"
        );
        }

        // A guest sees nothing of a link but that it is one, so its times are no more
        // its to set than where it points is to read.
        // @behavior FS-187
        #[test]
        fn a_link_keeps_its_own_times() {
            let host = host_with_a_file("link-time");
            std::os::unix::fs::symlink("dir", host.0.join("link")).expect("the link must be made");
            assert_eq!(
                do_utimes(
                    &Arc::default(),
                    &root_of(Arc::new(bind_of(&host))),
                    "link",
                    false,
                    &set_modified(),
                ),
                Err(FilesystemError::Unsupported),
                "vfs::do_utimes following no link of a Bind's link must answer ENOTSUP"
            );
        }

        // Another file takes the name, so a time reaching it by name would be seen.
        // @behavior FS-188
        #[test]
        fn a_time_set_through_a_descriptor_reaches_the_file_it_holds() {
            let host = host_with_a_file("held-time");
            let root = root_of(Arc::new(bind_of(&host)));
            let how = OpenHow {
                read: true,
                ..OpenHow::default()
            };
            let held = do_file_open(&Arc::default(), &root, "dir/file.txt", &how)
                .unwrap_or_else(|_| panic!("the file must open"));
            let file = host.0.join("dir/file.txt");
            fs::remove_file(&file).expect("the host removes the file");
            fs::write(&file, "y").expect("the host makes another");
            let other_before = host_modified(&file);
            let answer = do_utimes_fd(&Arc::default(), held.path(), &set_modified());
            let held_time = held.getattr().ok().and_then(|metadata| metadata.modified);
            assert_eq!(
            (answer, held_time, host_modified(&file) == other_before),
            (Ok(()), Some(moment()), true),
            "vfs::do_utimes_fd through a Bind's descriptor must set the file it holds, and leave \
             what took its name"
        );
        }
    }

    /// The host directory at `path` as a Bind, reached through the
    /// VFS.
    fn bind_host(path: impl AsRef<Path>) -> KernPath {
        kern_path(Arc::new(
            Bind::new(path).expect("the host directory must open"),
        ))
    }

    /// A fresh host directory of the test's own, removed when it drops.
    struct HostDir(PathBuf);

    impl HostDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("kobako-directory-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("the host directory must be created");
            Self(path)
        }
    }

    impl Drop for HostDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A Bind over `depth` nested directories each named `d`, with the file
    /// `f` at the bottom, and the path to that file.
    #[cfg(unix)]
    fn nested_tree(host: &HostDir, depth: usize) -> (Arc<Bind>, String) {
        let path = vec!["d"; depth].join("/");
        fs::create_dir_all(host.0.join(&path)).expect("the host tree must be made");
        fs::write(host.0.join(&path).join("f"), "x").expect("the host file must be written");
        let bind = Arc::new(Bind::new(&host.0).expect("the host directory must open"));
        (bind, format!("{path}/f"))
    }

    // Counted on the Bind, as a count of the process's descriptors would see
    // every other test's. Past the bound the walk still lands, so nothing a
    // guest reaches depends on what was kept.
    // @behavior FS-151
    #[cfg(unix)]
    #[test]
    fn a_walk_keeps_a_bounded_number_of_host_directories_open() {
        use crate::vfs::{self, OpenHow, Path as VfsPath};
        let host = HostDir::new("bounded-fids");
        let (bind, file) = nested_tree(&host, 200);
        let mount = crate::mount::Mount::new(
            "/data",
            crate::filesystem::Filesystem::new(bind.clone()).expect("the instance must stand"),
            crate::mount::Access::Read,
        )
        .expect("/data is a guest path");
        let root = VfsPath::root(mount.mnt());
        let read = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        let opened = vfs::do_file_open(&Arc::default(), &root, &file, &read);
        assert!(
            opened.is_ok() && bind.fids.len() <= 64,
            "vfs::do_file_open 200 directories beneath a Bind must land and keep at most 64 of \
             them open, keeping {}",
            bind.fids.len()
        );
    }

    // @behavior FS-196
    #[test]
    fn a_directory_of_each_built_in_filesystem_answers_its_read_as_a_directory() {
        use crate::vfs::File;
        let host = HostDir::new("reads-directory");
        let read = |fs: Arc<dyn FileSystemType>| {
            let mount = crate::mount::Mount::new(
                "/data",
                Filesystem::new(fs).expect("the instance must stand"),
                crate::mount::Access::Read,
            )
            .expect("/data is a guest path");
            let mut dir = File::root(mount.mnt(), &Arc::default()).expect("the root opens");
            (dir.read(&mut [0; 1]).err(), dir.pread(&mut [0; 1], 0).err())
        };
        let overlay = crate::filesystem::Overlay::new(
            vec![
                Filesystem::new(Arc::new(crate::filesystem::Tmpfs::new()))
                    .expect("the instance must stand"),
                Filesystem::new(Arc::new(crate::filesystem::Tmpfs::new()))
                    .expect("the instance must stand"),
            ],
            None,
        )
        .expect("the Overlay must stack");
        let refused = Some(FilesystemError::IsADirectory);
        for answer in [
            read(Arc::new(crate::filesystem::Tmpfs::new())),
            read(Arc::new(
                Bind::new(&host.0).expect("the host directory must open"),
            )),
            read(Arc::new(overlay)),
        ] {
            assert_eq!(
                answer,
                (refused.clone(), refused.clone()),
                "File::read and pread of a directory of each built-in Filesystem must answer \
                 EISDIR, as generic_read_dir does"
            );
        }
    }

    // Nothing holds `x` between the two stats, so what answers the second is the
    // negative dentry the first left, asked whether it still holds.
    // @behavior FS-193
    #[test]
    fn a_name_kept_as_absent_shows_what_the_host_made_there() {
        use crate::vfs::{self, Path as VfsPath};
        let host = HostDir::new("kept-absent");
        let mount = crate::mount::Mount::new(
            "/data",
            crate::filesystem::Filesystem::new(Arc::new(
                Bind::new(&host.0).expect("the host directory must open"),
            ))
            .expect("the instance must stand"),
            crate::mount::Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let root = VfsPath::root(mount.mnt());
        let cred = Arc::default();
        let stat = || vfs::vfs_fstatat(&cred, &root, "x", false).err();
        let before = stat();
        fs::write(host.0.join("x"), "x").expect("the host file must be written");
        assert_eq!(
            (before, stat()),
            (Some(FilesystemError::NotFound), None),
            "vfs::vfs_fstatat of a name kept as absent, once the host made a file there, must \
             find the file"
        );
    }

    // The Bind records nothing it makes, so what stands at the name is known only
    // to a lookup, which the absent dentry the walk kept must not answer for.
    // @behavior FS-194
    #[test]
    fn a_directory_made_where_a_name_was_kept_as_absent_is_found() {
        use crate::vfs::{self, Path as VfsPath};
        let host = HostDir::new("made-at-absent");
        let mount = crate::mount::Mount::new(
            "/data",
            crate::filesystem::Filesystem::new(Arc::new(
                Bind::new(&host.0).expect("the host directory must open"),
            ))
            .expect("the instance must stand"),
            crate::mount::Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let root = VfsPath::root(mount.mnt());
        let cred = Arc::default();
        let before = vfs::vfs_fstatat(&cred, &root, "made", false).err();
        vfs::filename_mkdirat(&cred, &root, "made").expect("the directory must be made");
        let after = vfs::vfs_fstatat(&cred, &root, "made", false).map(|made| made.file_type);
        assert_eq!(
            (before, after),
            (Some(FilesystemError::NotFound), Ok(FileType::Directory)),
            "vfs::filename_mkdirat at a name kept as absent must leave the next walk finding \
             the directory, as d_drop leaves a dentry nothing recorded on"
        );
    }

    // Nothing holds `x` between the two opens, so only a walk asking whether
    // the name it kept still stands finds the directory gone from beneath the
    // Mount, rather than following what it kept out through the moved one.
    // @behavior FS-161
    #[cfg(unix)]
    #[test]
    fn a_name_kept_for_the_next_walk_reaches_nothing_the_host_moved_out() {
        use crate::vfs::{self, OpenHow, Path as VfsPath};
        let host = HostDir::new("kept-name");
        fs::create_dir_all(host.0.join("m/x")).expect("the host tree must be made");
        fs::create_dir(host.0.join("outside")).expect("the host tree must be made");
        fs::write(host.0.join("m/x/secret"), "s").expect("the host file must be written");
        let mount = crate::mount::Mount::new(
            "/data",
            crate::filesystem::Filesystem::new(Arc::new(
                Bind::new(host.0.join("m")).expect("the host directory must open"),
            ))
            .expect("the instance must stand"),
            crate::mount::Access::Read,
        )
        .expect("/data is a guest path");
        let root = VfsPath::root(mount.mnt());
        let cred = crate::security::prepare_creds([&mount]);
        let read = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        let open = || vfs::do_file_open(&cred, &root, "x/secret", &read).err();
        let before = open();
        fs::rename(host.0.join("m/x"), host.0.join("outside/x")).expect("the host must move it");
        assert_eq!(
            (before, open()),
            (None, Some(FilesystemError::NotFound)),
            "vfs::do_file_open through a name kept since the last walk, once the host moved it \
             out of the Mount, must answer ENOENT"
        );
    }

    // Another writer moves the name the Policy judges, then moves a refused
    // file onto where it went, between the judgement and the open, so only
    // telling what opened from what was judged keeps the open from reaching it.
    // @behavior FS-156
    #[cfg(unix)]
    #[test]
    fn an_open_reaches_only_the_entry_it_judged() {
        use crate::vfs::{self, OpenHow, Path as VfsPath};

        /// Runs what it holds once, as a Policy is asked, then allows it.
        struct RunsWhenAsked(std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>);

        impl crate::mount::Policy for RunsWhenAsked {
            fn allows(&self, _path: &str, _access: crate::mount::Access) -> bool {
                let run = self.0.lock().expect("never poisoned").take();
                if let Some(run) = run {
                    run();
                }
                true
            }
        }

        let host = HostDir::new("open-identity");
        for dir in ["public", "archive"] {
            fs::create_dir(host.0.join(dir)).expect("the host tree must be made");
        }
        fs::write(host.0.join("public/x"), "judged").expect("the host file must be written");
        fs::write(host.0.join("archive/secret"), "refused").expect("the host file must be written");
        let filesystem = crate::filesystem::Filesystem::new(Arc::new(
            Bind::new(&host.0).expect("the host directory must open"),
        ))
        .expect("the instance must stand");
        let writer =
            crate::mount::Mount::new("/data", filesystem.clone(), crate::mount::Access::ReadWrite)
                .expect("/data is a guest path");
        let writer = VfsPath::root(writer.mnt());
        let swap = Box::new(move || {
            let cred = Arc::default();
            vfs::filename_renameat2(&cred, &writer, "public/x", &writer, "archive/x")
                .expect("the name must move");
            vfs::filename_renameat2(&cred, &writer, "archive/secret", &writer, "archive/x")
                .expect("the refused file must move onto it");
        });
        let mount = crate::mount::Mount::new("/data", filesystem, crate::mount::Access::Read)
            .expect("/data is a guest path")
            .with_policy(Arc::new(RunsWhenAsked(std::sync::Mutex::new(Some(swap)))));
        let read = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        let opened = vfs::do_file_open(
            &crate::security::prepare_creds([&mount]),
            &VfsPath::root(mount.mnt()),
            "public/x",
            &read,
        );
        assert_eq!(
            opened.err(),
            Some(FilesystemError::NotFound),
            "vfs::do_file_open of a file another writer replaced after the Policy judged it must \
             answer ENOENT rather than open what stands there now"
        );
    }

    // Other walks push what the descriptor walked to out of the bound, so only
    // a descriptor keeping its own directory still reaches it once the host
    // moved it.
    // @behavior FS-152
    #[cfg(unix)]
    #[test]
    fn a_directory_held_open_keeps_its_host_directory_past_the_bound() {
        use crate::vfs::{self, OpenHow, Path as VfsPath};
        let host = HostDir::new("held-past-bound");
        fs::create_dir_all(host.0.join("held")).expect("the host tree must be made");
        fs::write(host.0.join("held/f"), "x").expect("the host file must be written");
        let (bind, file) = nested_tree(&host, 100);
        let mount = crate::mount::Mount::new(
            "/data",
            crate::filesystem::Filesystem::new(bind).expect("the instance must stand"),
            crate::mount::Access::Read,
        )
        .expect("/data is a guest path");
        let root = VfsPath::root(mount.mnt());
        let cred = Arc::default();
        let directory = OpenHow {
            directory: true,
            ..OpenHow::default()
        };
        let Ok(held) = vfs::do_file_open(&cred, &root, "held", &directory) else {
            panic!("the directory must open");
        };
        let read = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        assert!(
            vfs::do_file_open(&cred, &root, &file, &read).is_ok(),
            "the deep file must open"
        );
        fs::rename(host.0.join("held"), host.0.join("moved")).expect("the host must move it");
        assert!(
            vfs::vfs_fstatat(&cred, held.path(), "f", false).is_ok(),
            "vfs::vfs_fstatat through a directory held open beneath a Bind, once a walk passed \
             the bound and the host moved it, must still reach the directory it opened"
        );
    }

    // The dentry the descriptor holds names the moved directory, so only a
    // Bind answering it stale keeps the removal of the new one from
    // killing it.
    // @behavior FS-135
    #[cfg(unix)]
    #[test]
    fn a_directory_remade_by_the_host_leaves_the_moved_ones_descriptor_alive() {
        use crate::mount::Mount;
        use crate::vfs::{self, OpenHow};
        let host = HostDir::new("remade");
        fs::create_dir_all(host.0.join("a")).expect("the host directory must be made");
        let filesystem = crate::filesystem::Filesystem::new(Arc::new(
            Bind::new(&host.0).expect("the host directory must open"),
        ))
        .expect("the instance must stand");
        let mount = Mount::new("/data", filesystem, crate::mount::Access::ReadWrite)
            .expect("/data is a guest path");
        let root = vfs::Path::root(mount.mnt());
        let how = OpenHow {
            directory: true,
            ..OpenHow::default()
        };
        let Ok(opened) = vfs::do_file_open(&Arc::default(), &root, "a", &how) else {
            panic!("the directory must open");
        };
        let held = opened.path();
        fs::rename(host.0.join("a"), host.0.join("moved")).expect("the host must move it");
        fs::create_dir(host.0.join("a")).expect("the host must remake it");
        vfs::filename_rmdir(&Arc::default(), &root, "a").expect("the new directory must go");
        assert_eq!(
            (
                vfs::filename_mkdirat(&Arc::default(),held, "made"),
                host.0.join("moved/made").is_dir()
            ),
            (Ok(()), true),
            "vfs::filename_mkdirat through a Bind directory the host moved away, once vfs::filename_rmdir \
             removed the one remade at its path, must make the directory in the moved one"
        );
    }

    /// A Policy refusing everything beneath `/data/refused`.
    #[cfg(unix)]
    struct RefusesRefused;

    #[cfg(unix)]
    impl crate::mount::Policy for RefusesRefused {
        fn allows(&self, path: &str, _access: crate::mount::Access) -> bool {
            !path.starts_with("/data/refused")
        }
    }

    // The guest's descriptor of `a/d` keeps `a`'s dentry, and the host
    // directory a walk opened for it, so only a dentry the Bind answers
    // stale once the host moved `a` keeps a later walk through `a` from
    // reaching the moved directory while the Policy judges `a`.
    // @behavior FS-136
    #[cfg(unix)]
    #[test]
    fn a_walk_through_a_held_directory_the_host_moved_reaches_nothing_the_policy_refuses() {
        use crate::mount::Mount;
        use crate::vfs::{self, OpenHow};
        let host = HostDir::new("held-ancestor");
        fs::create_dir_all(host.0.join("a/d")).expect("the host tree must be made");
        fs::create_dir_all(host.0.join("refused")).expect("the host tree must be made");
        fs::write(host.0.join("a/d/secret"), "s").expect("the host file must be written");
        let filesystem = crate::filesystem::Filesystem::new(Arc::new(
            Bind::new(&host.0).expect("the host directory must open"),
        ))
        .expect("the instance must stand");
        let mount = Mount::new("/data", filesystem, crate::mount::Access::Read)
            .expect("/data is a guest path")
            .with_policy(Arc::new(RefusesRefused));
        let root = vfs::Path::root(mount.mnt());
        let cred = crate::security::prepare_creds([&mount]);
        let directory = OpenHow {
            directory: true,
            ..OpenHow::default()
        };
        let Ok(_held) = vfs::do_file_open(&cred, &root, "a/d", &directory) else {
            panic!("a/d must open");
        };
        fs::rename(host.0.join("a"), host.0.join("refused/a")).expect("the host must move a");
        fs::create_dir(host.0.join("a")).expect("the host must remake a");
        let read = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        assert_eq!(
            vfs::do_file_open(&cred, &root, "a/d/secret", &read).err(),
            Some(FilesystemError::NotFound),
            "vfs::do_file_open beneath a Bind Mount with a Policy, through a directory the host moved \
             beneath a refused path while a descriptor beneath it was held, must answer NotFound"
        );
    }

    /// The roots of a Tmpfs, a Bind, and an Overlay, each holding a five byte
    /// `f`, by name.
    #[cfg(unix)]
    fn each_holding_five_bytes(host: &HostDir) -> [(&'static str, crate::vfs::Path); 3] {
        use crate::filesystem::Filesystem;
        use crate::filesystem::{Overlay, Tmpfs};
        use crate::mount::{Access, Mount};
        fs::write(host.0.join("f"), "hello").expect("the host file must be written");
        let tmpfs = || [("f", "hello")].into_iter().collect::<Tmpfs>();
        let overlay = Overlay::new(
            vec![Filesystem::new(Arc::new(tmpfs())).expect("the instance must stand")],
            Some(Filesystem::new(Arc::new(Tmpfs::new())).expect("the instance must stand")),
        )
        .expect("the Overlay must stack");
        let root = |filesystem: Arc<dyn FileSystemType>| {
            let mount = Mount::new(
                "/data",
                Filesystem::new(filesystem).expect("the instance must stand"),
                Access::ReadWrite,
            )
            .expect("/data is a guest path");
            crate::vfs::Path::root(mount.mnt())
        };
        [
            ("Tmpfs", root(Arc::new(tmpfs()))),
            (
                "Bind",
                root(Arc::new(
                    Bind::new(&host.0).expect("the host directory must open"),
                )),
            ),
            ("Overlay", root(Arc::new(overlay))),
        ]
    }

    // @behavior FS-141
    #[cfg(unix)]
    #[test]
    fn a_file_removed_while_open_still_reports_itself() {
        use crate::vfs::{self, OpenHow};
        let host = HostDir::new("removed-open");
        let read = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        for (name, root) in each_holding_five_bytes(&host) {
            let Ok(mut file) = vfs::do_file_open(&Arc::default(), &root, "f", &read) else {
                panic!("{name}'s f must open");
            };
            vfs::filename_unlinkat(&Arc::default(), &root, "f").expect("the name must go");
            let mut buf = [0; 5];
            assert_eq!(
                (
                    file.getattr().map(|found| found.len).ok(),
                    file.pread(&mut buf, 0),
                    &buf
                ),
                (Some(5), Ok(5), b"hello"),
                "File::getattr and File::pread of a file whose name \
                 vfs::filename_unlinkat removed on a {name} must answer what it holds"
            );
        }
    }

    // @behavior FS-142
    #[cfg(unix)]
    #[test]
    fn a_file_removed_while_open_can_still_be_sized() {
        use crate::vfs::{self, OpenHow};
        let host = HostDir::new("removed-sized");
        let write = OpenHow {
            write: true,
            ..OpenHow::default()
        };
        for (name, root) in each_holding_five_bytes(&host) {
            let Ok(mut file) = vfs::do_file_open(&Arc::default(), &root, "f", &write) else {
                panic!("{name}'s f must open");
            };
            vfs::filename_unlinkat(&Arc::default(), &root, "f").expect("the name must go");
            assert_eq!(
                (file.truncate(2), file.getattr().map(|found| found.len).ok()),
                (Ok(()), Some(2)),
                "File::truncate of a file whose name vfs::filename_unlinkat removed on a {name} \
                 must size what it holds"
            );
        }
    }

    // A dentry the host replaced is dropped by its identity, so the path
    // reaches the new file while the open one keeps its own descriptor.
    // @behavior FS-143
    #[cfg(unix)]
    #[test]
    fn a_file_replaced_while_open_answers_apart_from_its_path() {
        use crate::vfs::{self, OpenHow};
        let host = HostDir::new("replaced-open");
        let [_, (_, root), _] = each_holding_five_bytes(&host);
        let read = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        let Ok(file) = vfs::do_file_open(&Arc::default(), &root, "f", &read) else {
            panic!("f must open");
        };
        fs::rename(host.0.join("f"), host.0.join("aside")).expect("the host must move f");
        fs::write(host.0.join("f"), "longer!").expect("the host must remake f");
        assert_eq!(
            (
                vfs::vfs_fstatat(&Arc::default(), &root, "f", true)
                    .map(|found| found.len)
                    .ok(),
                file.getattr().map(|found| found.len).ok()
            ),
            (Some(7), Some(5)),
            "vfs::vfs_fstatat of a path the host replaced under an open File must answer the new \
             file, and File::getattr the one it holds"
        );
    }

    // The guest's descriptor of `a` keeps the Overlay's dentry and the
    // Bind's, whose host directory the host then moved aside; only the
    // Overlay asking the Bind's dentry drops its own for the `a` remade there.
    // @behavior FS-139
    #[cfg(unix)]
    #[test]
    fn an_overlay_takes_afresh_a_name_its_bind_layer_no_longer_holds() {
        use crate::filesystem::Filesystem;
        use crate::filesystem::{Overlay, Tmpfs};
        use crate::mount::{Access, Mount};
        use crate::vfs::{self, OpenHow};
        let host = HostDir::new("overlay-replaced");
        fs::create_dir(host.0.join("a")).expect("the host directory must be made");
        let lower = Filesystem::new(Arc::new(
            Bind::new(&host.0).expect("the host directory must open"),
        ))
        .expect("the instance must stand");
        let upper = Filesystem::new(Arc::new(Tmpfs::new())).expect("the instance must stand");
        let overlay = Overlay::new(vec![lower], Some(upper)).expect("the Overlay must stack");
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(overlay)).expect("the instance must stand"),
            Access::Read,
        )
        .expect("/data is a guest path");
        let root = vfs::Path::root(mount.mnt());
        let directory = OpenHow {
            directory: true,
            ..OpenHow::default()
        };
        let Ok(_held) = vfs::do_file_open(&Arc::default(), &root, "a", &directory) else {
            panic!("a must open");
        };
        fs::rename(host.0.join("a"), host.0.join("aside")).expect("the host must move a");
        fs::create_dir(host.0.join("a")).expect("the host must remake a");
        fs::write(host.0.join("a/f"), "new").expect("the host file must be written");
        let read = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        let Ok(mut file) = vfs::do_file_open(&Arc::default(), &root, "a/f", &read) else {
            panic!("a/f must open through the directory the host remade");
        };
        let mut buf = [0; 3];
        assert_eq!(
            (file.pread(&mut buf, 0), &buf),
            (Ok(3), b"new"),
            "vfs::do_file_open through an Overlay over a Bind whose host replaced a directory \
             a descriptor holds must read the file standing there now"
        );
    }

    // The host took the directory after the name was found, so only the open
    // asking the host answers that it is gone, rather than a descriptor that
    // fails at its first use.
    // @behavior FS-166
    #[test]
    fn a_directory_that_cannot_open_answers_why() {
        let host = HostDir::new("gone-before-open");
        fs::create_dir_all(host.0.join("a")).expect("the host directory must be made");
        let filesystem = crate::filesystem::Filesystem::new(Arc::new(
            Bind::new(&host.0).expect("the host directory must open"),
        ))
        .expect("the instance must stand");
        let dentries = filesystem.dentries();
        let root = dentries.root();
        let dir = dentries.alloc_parallel(&root, "a");
        dir.lookup_inode(&root).expect("the name must be found");
        fs::remove_dir(host.0.join("a")).expect("the host must remove it");
        let answer = dir
            .inode_ops()
            .expect("the name records its directory")
            .open(&dir, &OpenOptions::default());
        assert_eq!(
            answer.err(),
            Some(FilesystemError::NotFound),
            "InodeOperations::open of a Bind's directory the host removed must answer ENOENT"
        );
    }

    // A walk finds what stands at the names now, so only comparing it with what
    // the dentry recorded keeps another directory from answering for it.
    // @behavior FS-167
    #[cfg(unix)]
    #[test]
    fn a_directory_the_host_replaced_is_not_walked_to_in_its_place() {
        let host = HostDir::new("replaced-before-walk");
        fs::create_dir_all(host.0.join("a")).expect("the host directory must be made");
        let filesystem = crate::filesystem::Filesystem::new(Arc::new(
            Bind::new(&host.0).expect("the host directory must open"),
        ))
        .expect("the instance must stand");
        let dentries = filesystem.dentries();
        let root = dentries.root();
        let dir = dentries.alloc_parallel(&root, "a");
        dir.lookup_inode(&root).expect("the name must be found");
        fs::rename(host.0.join("a"), host.0.join("moved")).expect("the host must move it");
        fs::create_dir(host.0.join("a")).expect("the host must make another");
        assert_eq!(
            dir.getattr().err(),
            Some(FilesystemError::NotFound),
            "InodeOperations::getattr of a Bind directory the host replaced must answer ENOENT \
             rather than what stands there now"
        );
    }

    // A host's own rename takes no such flag, so only the refusal keeps a
    // call asking for one from moving the file as a plain rename would.
    // @behavior FS-163
    #[test]
    fn a_filesystem_taking_no_rename_flag_refuses_one() {
        let host = HostDir::new("rename-flag");
        fs::write(host.0.join("a"), "a").expect("the host file must be written");
        let filesystem = crate::filesystem::Filesystem::new(Arc::new(
            Bind::new(&host.0).expect("the host directory must open"),
        ))
        .expect("the instance must stand");
        let dentries = filesystem.dentries();
        let root = dentries.root();
        let dir = filesystem.root().expect("the root stands").ops();
        let answer = dir.rename(
            &dentries.alloc_parallel(&root, "a"),
            dir.as_ref(),
            &dentries.alloc_parallel(&root, "b"),
            Some(RenameFlag::NoReplace),
        );
        assert_eq!(
            (answer, host.0.join("a").exists()),
            (Err(FilesystemError::InvalidInput), true),
            "InodeOperations::rename of a Bind asked for a flag must answer EINVAL and move \
             nothing"
        );
    }

    // A directory a Bind has named but not opened yet is walked to by the
    // names its dentry stands at when first asked, so a move the dentry
    // followed meanwhile leads to it, as 9p's walk reads them then.
    #[test]
    fn a_directory_named_before_a_move_opens_where_its_dentry_went() {
        let host = HostDir::new("named-then-moved");
        fs::create_dir_all(host.0.join("a")).expect("the host directory must be made");
        fs::write(host.0.join("a/note"), "n").expect("the host file must be written");
        let filesystem = crate::filesystem::Filesystem::new(Arc::new(
            Bind::new(&host.0).expect("the host directory must open"),
        ))
        .expect("the instance must stand");
        let dentries = filesystem.dentries();
        let root = dentries.root();
        let a = dentries.alloc_parallel(&root, "a");
        fs::rename(host.0.join("a"), host.0.join("x")).expect("the host must move it");
        dentries.move_dentry(&root, "a", &root, "x");
        let note = dentries.alloc_parallel(&a, "note");
        assert!(
            matches!(
                filesystem.root().expect("the root stands").ops().lookup(&note),
                Ok(Lookup::Found(inode)) if inode.kind() == FileType::File
            ),
            "a name looked up through Bind in a directory not yet opened, once its dentry moved \
             with it, must be found where the dentry stands"
        );
    }

    /// A host directory of the test's own holding the FIFO `pipe`.
    #[cfg(unix)]
    fn make_fifo_host(name: &str) -> HostDir {
        let host = HostDir::new(name);
        let made = std::process::Command::new("mkfifo")
            .arg(host.0.join("pipe"))
            .status()
            .expect("mkfifo must run");
        assert!(made.success(), "the FIFO must be made");
        host
    }

    // Opening the FIFO with no writer would wait for one, so only an open
    // refused before it waits answers within the deadline; `/dev/null`
    // is a device every unix host holds.
    // @behavior FS-090
    #[cfg(unix)]
    #[test]
    fn a_directory_opens_only_a_file_or_a_directory() {
        let host = make_fifo_host("fifo-open");
        let fifo = bind_host(&host.0);
        let devices = bind_host("/dev");
        let (sender, answered) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let read = OpenOptions {
                read: true,
                ..OpenOptions::default()
            };
            let _ = sender.send([
                fifo.open("pipe", &read).err(),
                devices.open("null", &read).err(),
            ]);
        });
        assert_eq!(
            answered.recv_timeout(crate::sched::testing::MAX_WAIT),
            Ok([
                Some(FilesystemError::PermissionDenied),
                Some(FilesystemError::PermissionDenied)
            ]),
            "a FIFO and a device opened through Bind::open must be refused as permission \
             denied without waiting"
        );
    }

    // @behavior FS-091
    #[cfg(unix)]
    #[test]
    fn a_directory_reports_a_fifo_as_one() {
        let host = make_fifo_host("fifo-type");
        let directory = bind_host(&host.0);
        let listed: Vec<_> = directory
            .entries()
            .expect("the directory must list")
            .into_iter()
            .map(|entry| entry.file_type)
            .collect();
        assert_eq!(
            (
                directory
                    .symlink_metadata("pipe")
                    .map(|metadata| metadata.file_type),
                directory
                    .symlink_metadata("pipe")
                    .map(|metadata| metadata.file_type),
                listed
            ),
            (Ok(FileType::Fifo), Ok(FileType::Fifo), vec![FileType::Fifo]),
            "a FIFO looked up through Bind::metadata and symlink_metadata, and listed \
             through read_dir, must answer a FIFO"
        );
    }

    // `/dev/null` stands on every unix host, and a socket needs only its file
    // to stand here, so neither is opened.
    // @behavior FS-144
    #[cfg(unix)]
    #[test]
    fn a_directory_reports_the_kind_its_host_gives_a_special_file() {
        let host = HostDir::new("special");
        let _socket = std::os::unix::net::UnixListener::bind(host.0.join("sock"))
            .expect("the socket must be bound");
        let kind = |directory: KernPath, name| {
            directory
                .symlink_metadata(name)
                .map(|metadata| metadata.file_type)
        };
        assert_eq!(
            (
                kind(bind_host(&host.0), "sock"),
                kind(bind_host("/dev"), "null"),
            ),
            (Ok(FileType::Socket), Ok(FileType::CharDevice)),
            "Bind::symlink_metadata of a socket and of /dev/null must answer the kind the host \
             gives each"
        );
    }

    // @behavior FS-005
    #[test]
    fn a_directory_serves_the_files_of_its_host_directory() {
        let host = HostDir::new("serves");
        fs::write(host.0.join("data.csv"), "a,b").expect("the host file must be written");

        let directory = bind_host(&host.0);
        let mut file = directory
            .open(
                "data.csv",
                &OpenOptions {
                    read: true,
                    ..OpenOptions::default()
                },
            )
            .expect("the host file must open");
        let mut buf = [0; 16];
        let read = file
            .read_iter(&mut Kiocb::at(0), &mut buf)
            .expect("the host file must read");
        assert_eq!(
            &buf[..read],
            b"a,b",
            "a host file read through a Bind's open must read back its content"
        );
    }

    #[test]
    fn an_open_for_neither_reading_nor_writing_still_opens() {
        let host = HostDir::new("lookup");
        fs::write(host.0.join("seen"), "x").expect("the host file must be written");
        let directory = bind_host(&host.0);
        assert!(directory.open("seen", &OpenOptions::default()).is_ok());
    }

    // @behavior FS-006
    #[test]
    fn a_directory_writes_into_its_host_directory() {
        let host = HostDir::new("writes");
        let directory = bind_host(&host.0);
        directory
            .open_or_create(
                "out.txt",
                &OpenOptions {
                    write: true,
                    ..OpenOptions::default()
                },
            )
            .expect("a new file must be created")
            .write_iter(&mut Kiocb::at(0), b"written")
            .expect("the write must land");
        assert_eq!(
            fs::read(host.0.join("out.txt")).expect("the host file must exist"),
            b"written",
            "a file created through a Bind's open must land in the host directory"
        );
    }

    // The file is opened to write, so only the refusal keeps the append
    // from taking the two steps another writer could land between.
    // @behavior FS-093
    #[cfg(windows)]
    #[test]
    fn a_directory_on_windows_refuses_to_append() {
        let host = HostDir::new("append");
        fs::write(host.0.join("f"), "x").expect("the host file must be written");
        let directory = bind_host(&host.0);
        let mut file = directory
            .open(
                "f",
                &OpenOptions {
                    write: true,
                    ..OpenOptions::default()
                },
            )
            .expect("the host file must open");
        assert_eq!(
            (
                file.write_iter(&mut Kiocb::at(0).with_append(), b"y").err(),
                fs::read(host.0.join("f")).expect("the host file must read")
            ),
            (Some(FilesystemError::Unsupported), b"x".to_vec()),
            "an append through a Bind file on Windows must be refused as unsupported and \
             leave the file as it was"
        );
    }

    // The host file exists beneath `d`, so only the refusal can keep the
    // open from reaching it.
    // @behavior FS-029
    #[cfg(windows)]
    #[test]
    fn a_directory_on_windows_refuses_a_name_holding_a_backslash() {
        let host = HostDir::new("backslash");
        fs::create_dir_all(host.0.join("d")).expect("the host directory must be created");
        fs::write(host.0.join("d").join("f"), "x").expect("the host file must be written");
        let directory = bind_host(&host.0);
        assert!(
            matches!(
                directory.open(r"d\f", &OpenOptions::default()),
                Err(FilesystemError::InvalidInput)
            ),
            "a path holding a backslash opened through a Bind on Windows must be refused as \
             invalid input"
        );
    }

    // Judged on every host, as only Windows's own CI reaches the refusal, and
    // what the host would do with each name is what no other host can show.
    // @behavior FS-153
    #[test]
    fn a_name_windows_reads_as_another_is_told_apart() {
        let aliases = [
            "secret.",
            "secret ",
            "file:stream",
            r"d\f",
            "con",
            "NUL.txt",
            "conin$",
            "com1 .log",
            "a?",
            "a*",
            "a<",
            "a|",
            "a\"",
            "a\u{1}",
        ];
        let plain = ["secret", "file.txt", "console", "com10", ".hidden", "a.b.c"];
        assert_eq!(
            (
                aliases
                    .iter()
                    .filter(|name| !is_windows_alias(name))
                    .count(),
                plain.iter().filter(|name| is_windows_alias(name)).count(),
            ),
            (0, 0),
            "is_windows_alias must name every spelling Windows reads as another, and no plain name"
        );
    }

    // @behavior FS-153
    #[cfg(windows)]
    #[test]
    fn a_directory_on_windows_refuses_a_name_it_reads_as_another() {
        let host = HostDir::new("aliases");
        fs::write(host.0.join("secret"), "x").expect("the host file must be written");
        let directory = bind_host(&host.0);
        assert!(
            ["secret.", "secret:stream", "CON"]
                .iter()
                .all(|name| matches!(
                    directory.open(name, &OpenOptions::default()),
                    Err(FilesystemError::InvalidInput)
                )),
            "a name Windows reads as another, opened through a Bind on Windows, must be refused \
             as invalid input"
        );
    }

    // The host answers ENAMETOOLONG, which only a mapping naming it keeps
    // from reaching the guest as an EIO.
    // @behavior FS-065
    #[test]
    fn a_directory_answers_a_host_failure_as_the_error_naming_it() {
        let host = HostDir::new("names");
        let directory = bind_host(&host.0);
        let created = directory.open_or_create(
            &"x".repeat(300),
            &OpenOptions {
                write: true,
                ..OpenOptions::default()
            },
        );
        assert_eq!(
            created.map(drop),
            Err(FilesystemError::InvalidFilename),
            "a name past the host's limit created through Bind::open must fail as \
             InvalidFilename, not as an unnamed failure"
        );
    }

    // Which offset a host refuses depends on its filesystem: tmpfs, XFS and
    // overlay keep files up to `i64::MAX`. The host's own EFBIG, 27 on Linux
    // and macOS alike, is what the Bind must name.
    // @behavior FS-068
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_directory_answers_a_host_file_too_large_as_too_large() {
        assert_eq!(
            to_error(io::Error::from_raw_os_error(27)),
            FilesystemError::FileTooLarge,
            "a host EFBIG through Bind's error mapping must fail as FileTooLarge, \
             not as an unnamed failure"
        );
    }

    // A Bind's root is the directory cap-std opened for its path alone on Linux,
    // so a sync of the descriptor it holds would answer EBADF only there.
    // @behavior FS-171
    #[test]
    fn a_binds_root_syncs() {
        let host = HostDir::new("sync-root");
        let mount = crate::mount::Mount::new(
            "/data",
            Filesystem::new(Arc::new(
                Bind::new(&host.0).expect("the host directory must open"),
            ))
            .expect("the instance must stand"),
            crate::mount::Access::Read,
        )
        .expect("/data is a guest path");
        let Ok(mut root) = crate::vfs::File::root(mount.mnt(), &Arc::default()) else {
            panic!("the root must open");
        };
        assert_eq!(
            root.fsync(false),
            Ok(()),
            "File::fsync of a Bind's root must succeed, as Linux syncs a directory"
        );
    }

    // A rename moves a name out from under `openat2` only within a race, which
    // the Linux baseline meets at times and no test can stage, so the answer the
    // refusal reaches is held here.
    // @behavior FS-191
    #[test]
    fn a_name_carried_out_as_it_is_opened_answers_not_found() {
        let refused = io::Error::new(io::ErrorKind::PermissionDenied, "a path led outside");
        assert_eq!(
            to_error(refused),
            FilesystemError::NotFound,
            "cap-std's refusal of a single name leading out of a Bind's directory must answer \
             ENOENT, as a move carried it away"
        );
    }

    // Eviction leaves a walk to reach a directory from one far above it, so only
    // a path past what the host takes in one call shows it goes on by name.
    // @behavior FS-190
    #[cfg(unix)]
    #[test]
    fn a_directory_past_the_hosts_path_limit_is_reached() {
        let host = HostDir::new("past-path-max");
        let top = Dir::open_ambient_dir(&host.0, ambient_authority()).expect("the host directory");
        let names = vec!["n".repeat(200); 30];
        let deepest = names
            .iter()
            .fold(top.try_clone().expect("a second handle"), |at, name| {
                at.create_dir(name)
                    .expect("the host directory must be made");
                at.open_dir(name).expect("the host directory must open")
            });
        deepest
            .write("key", "x")
            .expect("the host file must be written");
        let reached = walk::walk(&top, &names).map(|dir| dir.exists("key"));
        assert_eq!(
            reached.ok(),
            Some(true),
            "a Bind's walk to a directory 6000 bytes beneath the one it holds must reach it, past \
             what the host takes in one call"
        );
    }

    // Root reads anything, so a host letting this one be read has nothing to
    // show. Linux reaches a directory for its path alone, so only asking to read
    // it there makes the host answer; macOS answers on any open.
    // @behavior FS-189
    #[cfg(unix)]
    #[test]
    fn a_directory_the_host_lets_no_one_read_refuses_an_open_to_read() {
        use std::os::unix::fs::PermissionsExt;

        let host = HostDir::new("unreadable");
        let dir = host.0.join("sealed");
        fs::create_dir(&dir).expect("the host directory must be made");
        let mode = |mode| fs::Permissions::from_mode(mode);
        fs::set_permissions(&dir, mode(0o000)).expect("the host directory's mode must be set");
        let enforced = fs::read_dir(&dir).is_err();
        let mount = crate::mount::Mount::new(
            "/data",
            Filesystem::new(Arc::new(
                Bind::new(&host.0).expect("the host directory must open"),
            ))
            .expect("the instance must stand"),
            crate::mount::Access::Read,
        )
        .expect("/data is a guest path");
        let root = crate::vfs::Path::root(mount.mnt());
        let how = crate::vfs::OpenHow {
            read: true,
            directory: true,
            ..Default::default()
        };
        let answer = crate::vfs::do_file_open(&Arc::default(), &root, "sealed", &how).err();
        fs::set_permissions(&dir, mode(0o755)).expect("the host directory's mode must be restored");
        if enforced {
            assert_eq!(
                answer,
                Some(FilesystemError::PermissionDenied),
                "vfs::do_file_open to read a host directory no one may read must answer EACCES"
            );
        }
    }

    // Linux holds a directory for its path alone, so only asking the host to
    // read it refuses one no one may read; macOS refuses it on any open. Root
    // reads anything, so only another user sees the refusal.
    // @behavior FS-200
    #[cfg(unix)]
    #[test]
    fn a_bind_of_a_directory_the_host_lets_no_one_read_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let host = HostDir::new("unreadable-root");
        let mode = |mode| fs::Permissions::from_mode(mode);
        fs::set_permissions(&host.0, mode(0o100)).expect("the host directory's mode must be set");
        let enforced = fs::read_dir(&host.0).is_err();
        let answer = Bind::new(&host.0).err().map(|err| err.kind());
        fs::set_permissions(&host.0, mode(0o755))
            .expect("the host directory's mode must be restored");
        if enforced {
            assert_eq!(
                answer,
                Some(io::ErrorKind::PermissionDenied),
                "Bind::new of a host directory no one may read must be refused as permission denied"
            );
        }
    }

    // macOS has no open for a directory's path alone. Root reads anything, so
    // only a walk run as another user sees the difference.
    // @behavior FS-172
    #[cfg(target_os = "linux")]
    #[test]
    fn a_directory_a_host_lets_be_searched_reports_itself() {
        use std::os::unix::fs::PermissionsExt;

        let host = HostDir::new("search-only");
        fs::create_dir(host.0.join("searched")).expect("the host directory must be made");
        let mode = |mode| fs::Permissions::from_mode(mode);
        fs::set_permissions(host.0.join("searched"), mode(0o111))
            .expect("the host directory's mode must be set");
        let mount = crate::mount::Mount::new(
            "/data",
            Filesystem::new(Arc::new(
                Bind::new(&host.0).expect("the host directory must open"),
            ))
            .expect("the instance must stand"),
            crate::mount::Access::Read,
        )
        .expect("/data is a guest path");
        let root = crate::vfs::Path::root(mount.mnt());
        let found = crate::vfs::vfs_fstatat(&Arc::default(), &root, "searched", false);
        fs::set_permissions(host.0.join("searched"), mode(0o755))
            .expect("the host directory's mode must be restored");
        assert_eq!(
            found.map(|metadata| metadata.file_type),
            Ok(FileType::Directory),
            "vfs::vfs_fstatat of a host directory only searchable must report it, as Linux's stat \
             needs no read permission on it"
        );
    }

    // Linux walks a directory it may only search, so a stat through one
    // succeeds there; the Bind reads the directory to learn the name's
    // spelling, which the mode refuses. Root reads anything, so only another
    // user sees the refusal.
    // @behavior FS-202
    #[cfg(unix)]
    #[test]
    fn a_name_beneath_a_directory_the_host_lets_only_be_searched_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let host = HostDir::new("search-only-name");
        fs::create_dir(host.0.join("searched")).expect("the host directory must be made");
        fs::write(host.0.join("searched/x"), "x").expect("the host file must be written");
        let mode = |mode| fs::Permissions::from_mode(mode);
        fs::set_permissions(host.0.join("searched"), mode(0o111))
            .expect("the host directory's mode must be set");
        let enforced = fs::read_dir(host.0.join("searched")).is_err();
        let mount = crate::mount::Mount::new(
            "/data",
            Filesystem::new(Arc::new(
                Bind::new(&host.0).expect("the host directory must open"),
            ))
            .expect("the instance must stand"),
            crate::mount::Access::Read,
        )
        .expect("/data is a guest path");
        let root = crate::vfs::Path::root(mount.mnt());
        let found = crate::vfs::vfs_fstatat(&Arc::default(), &root, "searched/x", false);
        fs::set_permissions(host.0.join("searched"), mode(0o755))
            .expect("the host directory's mode must be restored");
        if enforced {
            assert_eq!(
                found.err(),
                Some(FilesystemError::PermissionDenied),
                "vfs::vfs_fstatat of a file beneath a host directory only searchable must \
                 answer EACCES, as the Bind reads the directory to spell the name"
            );
        }
    }

    // Only Linux keeps a name that is not UTF-8; macOS refuses to make one.
    // @behavior FS-066
    #[cfg(target_os = "linux")]
    #[test]
    fn a_directory_refuses_to_list_a_host_name_that_is_not_utf8() {
        use std::os::unix::ffi::OsStrExt;

        let host = HostDir::new("ilseq");
        fs::write(host.0.join(std::ffi::OsStr::from_bytes(b"\xff")), "")
            .expect("the host file must be written");
        let directory = bind_host(&host.0);
        assert_eq!(
            directory.entries().map(drop),
            Err(FilesystemError::IllegalSequence),
            "a host name that is not UTF-8 listed through Bind::entries must fail as \
             IllegalSequence, not as an unnamed failure"
        );
    }

    // The host file only reads, so a host open asking to write, as one that
    // creates must, would be refused where nothing is to be created.
    // @behavior FS-064
    #[cfg(unix)]
    #[test]
    fn a_directory_opens_a_file_already_there_without_writing_to_create_it() {
        use std::os::unix::fs::PermissionsExt;
        let host = HostDir::new("create-existing");
        fs::write(host.0.join("note"), "x").expect("the host file must be written");
        fs::set_permissions(host.0.join("note"), fs::Permissions::from_mode(0o444))
            .expect("the host file must become read-only");
        let directory = bind_host(&host.0);
        let creates = OpenOptions {
            read: true,
            ..OpenOptions::default()
        };
        assert_eq!(
            (
                directory.open_or_create("note", &creates).map(drop),
                directory.open_or_create("new", &creates).map(drop),
                host.0.join("new").exists()
            ),
            (Ok(()), Ok(()), true),
            "a Bind::open that may create, of a read-only host file already there, must open \
             it, and of a missing one must create it"
        );
    }

    // The link's target is a readable file, so only the refusal of the
    // link itself keeps the open from reaching it.
    // @behavior FS-047
    #[cfg(unix)]
    #[test]
    fn a_directory_opens_no_link() {
        let host = HostDir::new("nofollow");
        fs::write(host.0.join("target"), "x").expect("the host file must be written");
        std::os::unix::fs::symlink("target", host.0.join("link")).expect("the link must be made");
        let opened = bind_host(&host.0)
            .open(
                "link",
                &OpenOptions {
                    read: true,
                    ..OpenOptions::default()
                },
            )
            .map(drop);
        assert_eq!(
            opened,
            Err(FilesystemError::FilesystemLoop),
            "a host link opened through a Bind must be refused as a link, its target unreached"
        );
    }

    // The target's five bytes would show as the link's size if a lookup
    // that follows none still followed it.
    // @behavior FS-048
    #[cfg(unix)]
    #[test]
    fn a_directory_reports_a_link_a_lookup_does_not_follow_as_a_link() {
        let host = HostDir::new("lstat");
        fs::write(host.0.join("target"), "hello").expect("the host file must be written");
        std::os::unix::fs::symlink("target", host.0.join("link")).expect("the link must be made");
        let directory = bind_host(&host.0);
        let listed = directory
            .entries()
            .expect("the host directory must list")
            .into_iter()
            .find(|entry| entry.name == "link")
            .map(|entry| entry.file_type);
        assert_eq!(
            (
                directory
                    .symlink_metadata("link")
                    .map(|metadata| metadata.file_type),
                listed
            ),
            (Ok(FileType::Symlink), Some(FileType::Symlink)),
            "a host link looked up through Bind::symlink_metadata and listed through \
             Bind::entries must report a link"
        );
    }

    // A directory's size is the host's to choose, so the host's own stat
    // is the only witness of it.
    // @behavior FS-138
    #[test]
    fn a_directory_reports_the_size_its_host_reports() {
        let host = HostDir::new("dir-size");
        fs::create_dir(host.0.join("sub")).expect("the host directory must be created");
        fs::write(host.0.join("sub/file"), "hello").expect("the host file must be written");
        let directory = bind_host(&host.0);
        assert_eq!(
            directory
                .symlink_metadata("sub")
                .map(|metadata| metadata.len),
            Ok(fs::metadata(host.0.join("sub"))
                .expect("the host directory must stand")
                .len()),
            "a host directory looked up through Bind::symlink_metadata must report the size its \
             host reports"
        );
    }

    // @behavior FS-028
    #[test]
    fn a_directory_refuses_to_remove_a_directory_as_a_file() {
        let host = HostDir::new("unlink-dir");
        fs::create_dir(host.0.join("sub")).expect("the host directory must be created");
        let directory = bind_host(&host.0);
        assert_eq!(
            directory.remove_file("sub"),
            Err(FilesystemError::IsADirectory),
            "a host directory removed through Bind::remove_file must answer IsADirectory"
        );
    }

    /// A host directory holding `dir/sub/y` and `secret`, with `dir/in`
    /// linking to `sub` and `dir/up` to `../secret`.
    #[cfg(unix)]
    fn make_links(name: &str) -> HostDir {
        let host = HostDir::new(name);
        fs::create_dir_all(host.0.join("dir/sub")).expect("the host directory must be created");
        fs::write(host.0.join("dir/sub/y"), "inner").expect("the inner file must be written");
        fs::write(host.0.join("secret"), "outer").expect("the outer file must be written");
        std::os::unix::fs::symlink("sub", host.0.join("dir/in")).expect("the link must be made");
        std::os::unix::fs::symlink("../secret", host.0.join("dir/up"))
            .expect("the link must be made");
        host
    }

    // The link leads to a directory inside the host directory, so only
    // refusing the link itself keeps it from being opened.
    // @behavior FS-099
    #[cfg(unix)]
    #[test]
    fn a_directory_opens_no_link_as_a_directory_where_asked_not_to_follow() {
        let host = make_links("open-dir-nofollow");
        let directory = bind_host(&host.0);
        let dir = directory.open_dir("dir").expect("the directory must open");
        assert_eq!(
            dir.open_dir("in").map(drop),
            Err(FilesystemError::FilesystemLoop),
            "a link to a directory opened through Bind::open_dir must be a link loop"
        );
    }

    // @behavior FS-114
    #[cfg(unix)]
    #[test]
    fn a_directory_reads_where_a_link_points() {
        let host = make_links("read-link");
        let directory = bind_host(&host.0);
        let dir = directory.open_dir("dir").expect("the directory must open");
        assert_eq!(
            (dir.read_link("up"), dir.read_link("sub")),
            (
                Ok("../secret".to_string()),
                Err(FilesystemError::InvalidInput)
            ),
            "Bind::read_link must answer where a link points as stored, and refuse a \
             directory as invalid input"
        );
    }

    // @behavior FS-007
    #[cfg(unix)]
    #[test]
    fn a_directory_reaches_nothing_outside_its_host_directory() {
        let outside = HostDir::new("outside");
        fs::write(outside.0.join("secret"), "host").expect("the outside file must be written");
        let host = HostDir::new("confined");
        std::os::unix::fs::symlink(outside.0.join("secret"), host.0.join("escape"))
            .expect("the link must be created");

        let directory = bind_host(&host.0);
        assert!(
            directory
                .open(
                    "escape",
                    &OpenOptions {
                        read: true,
                        ..OpenOptions::default()
                    }
                )
                .is_err(),
            "a link out of the host directory opened through a Bind's open must be refused"
        );
    }

    // @behavior FS-071
    #[cfg(unix)]
    #[test]
    fn a_directory_reports_the_identity_its_host_gives_a_path() {
        use std::os::unix::fs::MetadataExt;

        let host = HostDir::new("identity");
        fs::create_dir(host.0.join("dir")).expect("the host directory must be made");
        let expected = fs::metadata(host.0.join("dir")).expect("the host must stat it");
        let directory = bind_host(&host.0);
        let reported = directory
            .symlink_metadata("dir")
            .expect("the directory must stand");
        assert_eq!(
            (reported.dev, reported.ino),
            (expected.dev(), expected.ino()),
            "a host directory's metadata through Bind::metadata must carry the device and \
             inode numbers the host reports"
        );
    }

    // Two handles racing one-byte appends: a two-step append (read the
    // length, then write there) loses about half the bytes to the other
    // handle's writes, so only an append the host places keeps them all.
    // @behavior FS-069
    #[cfg(unix)]
    #[test]
    fn a_directory_appends_at_its_host_files_end_in_one_step() {
        let host = HostDir::new("append");
        fs::write(host.0.join("log.txt"), "").expect("the host file must be written");
        let directory = bind_host(&host.0);

        let writers: Vec<_> = [b"a", b"b"]
            .into_iter()
            .map(|byte| {
                let directory = directory.clone();
                std::thread::spawn(move || {
                    let options = OpenOptions {
                        write: true,
                        ..OpenOptions::default()
                    };
                    let mut file = directory
                        .open("log.txt", &options)
                        .expect("the host file must open to write");
                    for _ in 0..10_000 {
                        file.write_iter(&mut Kiocb::at(0).with_append(), byte)
                            .expect("the append must succeed");
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().expect("the writer must finish");
        }

        let written = fs::read(host.0.join("log.txt")).expect("the host file must be read");
        assert_eq!(
            (
                written.iter().filter(|&&b| b == b'a').count(),
                written.iter().filter(|&&b| b == b'b').count()
            ),
            (10_000, 10_000),
            "two handles appending to one host file through a Bind's FileOperations::write_iter must each \
             keep every byte"
        );
    }

    // ext4 hands a removed directory's inode number to the next one made,
    // so the number staying unused while open is what the opened directory
    // keeps. A host that never reuses numbers cannot show that, but every
    // Linux host lists the removed directory among the process's
    // descriptors while it is open.
    // @behavior FS-086
    #[cfg(target_os = "linux")]
    #[test]
    fn a_directory_opened_keeps_its_inode_number_from_the_next_one_made() {
        use std::os::unix::fs::MetadataExt;

        let host = HostDir::new("held");
        let ino = |path: &str| {
            fs::metadata(host.0.join(path))
                .expect("it must stand")
                .ino()
        };
        fs::create_dir(host.0.join("dir")).expect("the host directory must be made");
        let first = ino("dir");
        fs::remove_dir(host.0.join("dir")).expect("the host directory must go");
        fs::create_dir(host.0.join("dir")).expect("the host directory must be made");
        let reuses = ino("dir") == first;
        let first = ino("dir");
        let directory = bind_host(&host.0);
        let opened = directory.open_dir("dir").expect("the directory must open");
        fs::remove_dir(host.0.join("dir")).expect("the host directory must go");
        let removed = format!("{} (deleted)", host.0.join("dir").display());
        let held_open = fs::read_dir("/proc/self/fd")
            .expect("the process's descriptors must list")
            .filter_map(|entry| fs::read_link(entry.ok()?.path()).ok())
            .any(|target| target.to_string_lossy() == removed);
        fs::create_dir(host.0.join("dir")).expect("the host directory must be made");
        let made = ino("dir");
        drop(opened);
        assert!(
            held_open,
            "a directory Bind::open_dir opened must stay open once removed"
        );
        if !reuses {
            eprintln!("the host does not reuse inode numbers; nothing more to witness");
            return;
        }
        assert_ne!(
            made, first,
            "a directory made where Bind::open_dir holds a removed one open must take \
             another inode number on a host reusing them"
        );
    }

    // Windows refuses to move a directory a handle holds open, so the host
    // cannot make the move this witnesses there.
    // @behavior FS-108
    #[cfg(unix)]
    #[test]
    fn a_directory_opened_from_a_directory_follows_it_when_the_host_moves_it() {
        let host = HostDir::new("opened-moves");
        fs::create_dir(host.0.join("dir")).expect("the host directory must be made");
        let directory = bind_host(&host.0);
        let dir = directory
            .open_dir("dir")
            .expect("the host directory must open");
        fs::rename(host.0.join("dir"), host.0.join("moved")).expect("the host must move it");
        dir.open_or_create(
            "made",
            &OpenOptions {
                write: true,
                ..OpenOptions::default()
            },
        )
        .expect("the file must be made");
        assert_eq!(
            (
                host.0.join("moved/made").is_file(),
                host.0.join("dir").exists()
            ),
            (true, false),
            "a file made through a directory Bind::open_dir opened before the host moved it \
             must stand beneath the directory's new host path"
        );
    }

    // @behavior FS-081
    #[test]
    fn a_directory_spells_names_it_matches_exactly_as_given() {
        let host = HostDir::new("spelled");
        fs::create_dir(host.0.join("dir")).expect("the host directory must be made");
        fs::write(host.0.join("dir/file"), "x").expect("the host file must be written");
        let directory = bind_host(&host.0);
        let dir = directory
            .open_dir("dir")
            .expect("the host directory must open");
        assert_eq!(
            (dir.lookup("file"), dir.lookup("new")),
            (Ok("file".to_string()), Ok("new".to_string())),
            "a name asked of Bind::lookup in the spelling its host stores, and one it \
             does not hold, must come back as given"
        );
    }

    // macOS volumes fold case and Unicode form unless formatted
    // case-sensitive, where every spelling is already the stored one.
    // @behavior FS-082
    #[cfg(target_os = "macos")]
    #[test]
    fn a_directory_spells_a_path_as_a_folding_host_stores_it() {
        let host = HostDir::new("folded");
        fs::create_dir(host.0.join("private")).expect("the host directory must be made");
        fs::write(host.0.join("private/caf\u{e9}"), "x").expect("the host file must be written");
        if !host.0.join("PRIVATE").exists() {
            eprintln!("the host volume matches names exactly; nothing folds to witness");
            return;
        }
        let directory = bind_host(&host.0);
        let private = directory
            .open_dir("PRIVATE")
            .expect("the host directory must open in any case");
        assert_eq!(
            (
                directory.lookup("Private"),
                private.lookup("CAFE\u{301}"),
                private.lookup("missing")
            ),
            (
                Ok("private".to_string()),
                Ok("caf\u{e9}".to_string()),
                Ok("missing".to_string())
            ),
            "a name asked of Bind::lookup in another case and Unicode form on a host \
             folding both must come back as the host stores it, one it does not hold as given"
        );
    }
}
