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

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions as CapOpenOptions};

use super::{
    DentryOperations, DirEntry, FileOperations, FileType, FilesystemError, Iattr, InodeOperations,
    Kiocb, Lookup, Metadata, OpenOptions, RenameFlag,
};
use crate::vfs::{Dentry, Identity, Inode};
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
    /// reaches outside itself.
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            root: Arc::new(Dir::open_ambient_dir(path, ambient_authority())?),
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
    /// with what the dentry recorded instead, as NFS compares a file handle,
    /// and walks again from the root where the dentry moved meanwhile.
    fn fid(&self, dentry: &Dentry) -> Result<Arc<Dir>, FilesystemError> {
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
        let mut dir = walk::walk(&above, &names);
        if !dir.as_ref().is_ok_and(stands) {
            let path = dentry.path_raw();
            let names: Vec<String> = path.split('/').map(String::from).collect();
            dir = walk::walk(&self.root, &names);
        }
        let dir = dir?;
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
        let dir = match dentry.parent() {
            Some(parent) => self.fid(&parent)?,
            None => self.root.clone(),
        };
        call(&dir, to_host_name(&dentry.name())?)
    }

    /// What stands at the name `dentry` carries, spelled as the host spells
    /// it, as a lookup hands `d_splice_alias` the inode it found, so
    /// `revalidate` can tell a dentry the host replaced behind the
    /// runtime's back. That costs one host stat a name.
    fn lookup_at(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
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
        let Some(inode) = dentry.inode().filter(|inode| inode.identity().is_some()) else {
            return Ok(true);
        };
        let found = self.at(dentry, |dir, name| {
            dir.symlink_metadata(name).map_err(to_error)
        });
        match found {
            Ok(found) => {
                let found = to_metadata(found);
                Ok(Identity::of(&found) == inode.identity() && found.file_type == inode.kind())
            }
            Err(
                FilesystemError::NotFound
                | FilesystemError::NotADirectory
                | FilesystemError::FilesystemLoop,
            ) => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn entries_of(&self, dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError> {
        let dir = self.fid(dentry)?;
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
                let listed =
                    DirEntry::new(name, to_file_type(entry.file_type().map_err(to_error)?));
                #[cfg(unix)]
                let listed = {
                    use std::os::unix::fs::DirEntryExt;
                    listed.with_identity(dev, entry.ino())
                };
                Ok(listed)
            })
            .collect()
    }
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

/// The FilesystemError a host failure names. cap-std's own refusal of a
/// path leading out of the directory carries no OS code, and answers as
/// not permitted, as wasmtime-wasi reads it. A link loop stays unnamed
/// until std's `FilesystemLoop` is stable.
fn to_error(err: io::Error) -> FilesystemError {
    match err.kind() {
        io::ErrorKind::PermissionDenied if err.raw_os_error().is_none() => {
            FilesystemError::NotPermitted
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

/// The descriptors open on one file, and which were opened to write, as 9p
/// keeps open fids on `i_private`.
#[derive(Default)]
struct OpenFids(Mutex<Vec<(Weak<std::fs::File>, bool)>>);

/// A descriptor open on the file `dentry` records, one open to write where
/// `writable` asks, as 9p's `v9fs_fid_find_inode` finds one.
fn open_fid(dentry: &Dentry, writable: bool) -> Option<Arc<std::fs::File>> {
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
            .is_none_or(|inode| inode.identity().is_none())
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
        match open_fid(dentry, false) {
            Some(file) => Ok(to_metadata(
                cap_std::fs::Metadata::from_file(&file).map_err(to_error)?,
            )),
            None => stat_at(&self.0, dentry),
        }
    }

    fn setattr(&self, dentry: &Dentry, attr: &Iattr) -> Result<(), FilesystemError> {
        let Some(len) = attr.size else {
            return Err(FilesystemError::Unsupported);
        };
        match open_fid(dentry, true) {
            Some(file) => file.set_len(len).map_err(to_error),
            None => Err(FilesystemError::InvalidInput),
        }
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

    fn open(
        &self,
        _dentry: &Dentry,
        _options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        Err(FilesystemError::FilesystemLoop)
    }
}

impl InodeOperations for Bind {
    /// A directory answers from its own descriptor, which follows it wherever
    /// it moves and keeps its inode taken while it is open.
    fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        Ok(to_metadata(
            self.fid(dentry)?.dir_metadata().map_err(to_error)?,
        ))
    }

    fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
        self.lookup_at(dentry)
    }

    fn dentry_ops(&self) -> Option<Arc<dyn DentryOperations>> {
        Some(Arc::new(self.clone()))
    }

    fn open(
        &self,
        dentry: &Dentry,
        _options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let fid = self.fid(dentry)?;
        OpenDirFid::set(dentry, &fid);
        Ok(Box::new(BindDir(self.clone(), fid)))
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

/// A host directory, open: it reaches the descriptor kept on the dentry it is
/// handed, so it follows the directory wherever it moves. It also keeps the
/// descriptor it opened with, for a sync, which is handed no dentry.
struct BindDir(Bind, Arc<Dir>);

impl FileOperations for BindDir {
    fn iterate_shared(&mut self, dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError> {
        self.0.entries_of(dentry)
    }

    /// Make the host directory's names durable. Windows syncs no directory, so
    /// there the host answers for them itself.
    fn fsync(&mut self) -> Result<(), FilesystemError> {
        sync_dir(&self.1)
    }
}

#[cfg(unix)]
fn sync_dir(dir: &Dir) -> Result<(), FilesystemError> {
    rustix::fs::fsync(dir).map_err(|err| to_error(io::Error::from(err)))
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Dir) -> Result<(), FilesystemError> {
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
    fn read_iter(&mut self, iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        let mut file = &*self.file;
        file.seek(SeekFrom::Start(iocb.pos)).map_err(to_error)?;
        let read = file.read(buf).map_err(to_error)?;
        iocb.pos += read as u64;
        Ok(read)
    }

    fn write_iter(&mut self, iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
        if iocb.append {
            return self.append(iocb, buf);
        }
        let mut file = &*self.file;
        file.seek(SeekFrom::Start(iocb.pos)).map_err(to_error)?;
        let written = file.write(buf).map_err(to_error)?;
        iocb.pos += written as u64;
        Ok(written)
    }

    fn fsync(&mut self) -> Result<(), FilesystemError> {
        self.file.sync_all().map_err(to_error)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::vfs::kern_path::{kern_path, KernPath};

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
            crate::filesystem::Filesystem::new(bind.clone()),
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
            )),
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
            (
                None,
                Some(vfs::Error::Filesystem(FilesystemError::NotFound))
            ),
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
        ));
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
            Some(vfs::Error::Filesystem(FilesystemError::NotFound)),
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
            crate::filesystem::Filesystem::new(bind),
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
        ));
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
        ));
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
            Some(vfs::Error::Filesystem(FilesystemError::NotFound)),
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
            vec![Filesystem::new(Arc::new(tmpfs()))],
            Some(Filesystem::new(Arc::new(Tmpfs::new()))),
        )
        .expect("the Overlay must stack");
        let root = |filesystem: Arc<dyn InodeOperations>| {
            let mount = Mount::new("/data", Filesystem::new(filesystem), Access::ReadWrite)
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
        ));
        let upper = Filesystem::new(Arc::new(Tmpfs::new()));
        let overlay = Overlay::new(vec![lower], Some(upper)).expect("the Overlay must stack");
        let mount = Mount::new("/data", Filesystem::new(Arc::new(overlay)), Access::Read)
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
        ));
        let dentries = filesystem.dentries();
        let root = dentries.root();
        let dir = dentries.child(&root, "a");
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
        ));
        let dentries = filesystem.dentries();
        let root = dentries.root();
        let dir = dentries.child(&root, "a");
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
        ));
        let dentries = filesystem.dentries();
        let root = dentries.root();
        let answer = filesystem.root().rename(
            &dentries.child(&root, "a"),
            filesystem.root().as_ref(),
            &dentries.child(&root, "b"),
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
        ));
        let dentries = filesystem.dentries();
        let root = dentries.root();
        let a = dentries.child(&root, "a");
        fs::rename(host.0.join("a"), host.0.join("x")).expect("the host must move it");
        dentries.move_dentry(&root, "a", &root, "x");
        let note = dentries.child(&a, "note");
        assert!(
            matches!(
                filesystem.root().lookup(&note),
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
            answered.recv_timeout(std::time::Duration::from_secs(5)),
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
