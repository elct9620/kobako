//! `Directory`: a Filesystem over a host directory, from which nothing
//! outside it is reachable.
//!
//! Every path resolves through `cap_std::fs::Dir`, which refuses a `..`
//! or a symbolic link that would lead out of the directory, so the
//! confinement holds whatever path a caller hands over.

mod spelling;

use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions as CapOpenOptions};

use super::{
    DirEntry, File, FileType, Filesystem, FilesystemError, Hold, Metadata, OpenOptions, Rename,
};

/// A host directory and everything beneath it. What a guest writes lands
/// on the host and lasts as long as the directory does.
pub struct Directory {
    dir: Dir,
}

impl Directory {
    /// Open the host directory at `path`, the one place this Filesystem
    /// reaches outside itself.
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            dir: Dir::open_ambient_dir(path, ambient_authority())?,
        })
    }
}

/// `path` as `cap_std` takes it, the root being `.`. Windows reads `\` as
/// a separator too, so there a name holding one is refused rather than
/// reached as names a Mount's Policy never saw.
fn to_host_path(path: &str) -> Result<&str, FilesystemError> {
    if cfg!(windows) && path.contains('\\') {
        return Err(FilesystemError::InvalidInput);
    }
    Ok(if path.is_empty() { "." } else { path })
}

/// The FilesystemError a host failure names. A link loop stays unnamed
/// until std's `FilesystemLoop` is stable.
fn to_error(err: io::Error) -> FilesystemError {
    match err.kind() {
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
        FileType::Unknown
    }
}

/// What a host entry is, a file's size alone counted: a link reports no
/// size, so nothing of where it points reaches the guest. The host names
/// its identity on unix; elsewhere a Directory reports none.
fn to_metadata(metadata: cap_std::fs::Metadata) -> Metadata {
    let file_type = to_file_type(metadata.file_type());
    let len = if file_type == FileType::File {
        metadata.len()
    } else {
        0
    };
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        Metadata::new(file_type, len).with_identity(metadata.dev(), metadata.ino())
    }
    #[cfg(not(unix))]
    Metadata::new(file_type, len)
}

impl Filesystem for Directory {
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        // The host creates only for a writer, so a file already there is
        // opened as asked rather than for writing it was never given.
        if options.create && !(options.create_new || options.write || options.truncate) {
            let existing = OpenOptions {
                create: false,
                ..*options
            };
            match self.open(path, &existing) {
                Err(FilesystemError::NotFound) => {}
                opened => return opened,
            }
        }
        let creates = options.create || options.create_new || options.truncate;
        let writes = options.write || creates;
        let mut open = CapOpenOptions::new();
        // The host refuses an open for neither reading nor writing, and one
        // that creates or truncates without writing; the runtime decides
        // what is read and written, so such opens read or write here.
        open.read(options.read || !writes)
            .write(writes)
            .create(options.create)
            .create_new(options.create_new)
            .truncate(options.truncate);
        // Only a file or a directory opens: a FIFO would wait for a peer
        // and a device would act on being opened. The host could swap an
        // entry in between, so what opened is checked again before its
        // first use, the open itself never waiting for a peer.
        let found = if options.nofollow {
            self.dir.symlink_metadata(to_host_path(path)?)
        } else {
            self.dir.metadata(to_host_path(path)?)
        };
        match found.map(|metadata| to_file_type(metadata.file_type())) {
            Ok(FileType::Symlink) => return Err(FilesystemError::FilesystemLoop),
            Ok(FileType::Unknown) => return Err(FilesystemError::PermissionDenied),
            _ => {}
        }
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            use rustix::fs::OFlags;
            open.custom_flags((OFlags::NONBLOCK | OFlags::NOCTTY).bits() as i32);
        }
        let file = self
            .dir
            .open_with(to_host_path(path)?, &open)
            .map_err(to_error)?;
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
        Ok(Box::new(DirectoryFile {
            file: file.into_std(),
        }))
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        Ok(to_metadata(
            self.dir.metadata(to_host_path(path)?).map_err(to_error)?,
        ))
    }

    fn symlink_metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        Ok(to_metadata(
            self.dir
                .symlink_metadata(to_host_path(path)?)
                .map_err(to_error)?,
        ))
    }

    /// The host directory's own descriptor, so its inode stays taken.
    /// Elsewhere a Directory reports no identity, so it holds nothing.
    fn hold_directory(&self, path: &str) -> Result<Option<Hold>, FilesystemError> {
        if cfg!(not(unix)) {
            return Ok(None);
        }
        let dir = self.dir.open_dir(to_host_path(path)?).map_err(to_error)?;
        Ok(Some(Hold::new(dir)))
    }

    fn stored_path(&self, path: &str) -> Result<String, FilesystemError> {
        let mut parent = Some(self.dir.try_clone().map_err(to_error)?);
        let mut stored = Vec::new();
        let mut names = path.split('/').filter(|name| !name.is_empty()).peekable();
        while let Some(name) = names.next() {
            let found = match &parent {
                Some(dir) => spelling::stored_name(dir, to_host_path(name)?).map_err(to_error)?,
                None => None,
            };
            // The last name is spelled from its parent alone, so a link
            // standing there is never followed to spell it.
            parent = match (&parent, &found) {
                (Some(dir), Some(found)) if names.peek().is_some() => {
                    match dir.open_dir(found).map_err(to_error) {
                        Ok(next) => Some(next),
                        Err(FilesystemError::NotFound | FilesystemError::NotADirectory) => None,
                        Err(err) => return Err(err),
                    }
                }
                _ => None,
            };
            stored.push(found.unwrap_or_else(|| name.to_string()));
        }
        Ok(stored.join("/"))
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        self.dir
            .read_dir(to_host_path(path)?)
            .map_err(to_error)?
            .map(|entry| {
                let entry = entry.map_err(to_error)?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| FilesystemError::IllegalSequence)?;
                Ok(DirEntry {
                    name,
                    file_type: to_file_type(entry.file_type().map_err(to_error)?),
                })
            })
            .collect()
    }

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.dir.create_dir(to_host_path(path)?).map_err(to_error)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.dir.remove_dir(to_host_path(path)?).map_err(to_error)
    }

    /// A directory is refused as one on every host, where the host itself
    /// answers EISDIR on Linux but EPERM on macOS.
    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        if self
            .dir
            .symlink_metadata(to_host_path(path)?)
            .is_ok_and(|m| m.is_dir())
        {
            return Err(FilesystemError::IsADirectory);
        }
        self.dir.remove_file(to_host_path(path)?).map_err(to_error)
    }

    /// A host directory takes no lock a Filesystem could check the source
    /// under, so another process of the host's changing it in between is
    /// the host's to rule out. A link moves as itself, as `rename(2)`
    /// moves one.
    fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
        if from != to {
            rename.check(self.symlink_metadata(from)?.file_type)?;
        }
        self.dir
            .rename(to_host_path(from)?, &self.dir, to_host_path(to)?)
            .map_err(to_error)
    }

    /// Every Invocation shares the host directory.
    fn for_invocation(&self) -> Option<Arc<dyn Filesystem>> {
        None
    }
}

/// An open host file. The runtime names every offset, so each access seeks
/// first.
struct DirectoryFile {
    file: std::fs::File,
}

impl File for DirectoryFile {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        self.file.seek(SeekFrom::Start(offset)).map_err(to_error)?;
        self.file.read(buf).map_err(to_error)
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        self.file.seek(SeekFrom::Start(offset)).map_err(to_error)?;
        self.file.write(buf).map_err(to_error)
    }

    /// Windows has no append to switch one handle to for a single write,
    /// so an append that could land over another writer's is refused.
    #[cfg(not(unix))]
    fn append(&mut self, _buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        Err(FilesystemError::Unsupported)
    }

    /// The host places the write at the file's end as `O_APPEND` does, so
    /// another writer's append lands before or after it, never over it.
    #[cfg(unix)]
    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        use rustix::fs::{fcntl_getfl, fcntl_setfl, OFlags};

        let flags = fcntl_getfl(&self.file)
            .map_err(io::Error::from)
            .map_err(to_error)?;
        fcntl_setfl(&self.file, flags | OFlags::APPEND)
            .map_err(io::Error::from)
            .map_err(to_error)?;
        let written = self.file.write(buf);
        let end = self.file.stream_position();
        fcntl_setfl(&self.file, flags)
            .map_err(io::Error::from)
            .map_err(to_error)?;
        let written = written.map_err(to_error)?;
        Ok((end.map_err(to_error)? - written as u64, written))
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        let metadata = self.file.metadata().map_err(to_error)?;
        Ok(Metadata::new(FileType::File, metadata.len()))
    }

    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
        self.file.set_len(len).map_err(to_error)
    }

    fn sync(&mut self) -> Result<(), FilesystemError> {
        self.file.sync_all().map_err(to_error)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

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

    /// A host directory of the test's own holding the FIFO `pipe`.
    #[cfg(unix)]
    fn holding_a_fifo(name: &str) -> HostDir {
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
        let host = holding_a_fifo("fifo-open");
        let fifo = Directory::new(&host.0).expect("the host directory must open");
        let devices = Directory::new("/dev").expect("the device directory must open");
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
            "a FIFO and a device opened through Directory::open must be refused as permission \
             denied without waiting"
        );
    }

    // @behavior FS-091
    #[cfg(unix)]
    #[test]
    fn a_directory_reports_a_fifo_as_of_an_unknown_type() {
        let host = holding_a_fifo("fifo-type");
        let directory = Directory::new(&host.0).expect("the host directory must open");
        let listed: Vec<_> = directory
            .read_dir("")
            .expect("the directory must list")
            .into_iter()
            .map(|entry| entry.file_type)
            .collect();
        assert_eq!(
            (
                directory
                    .metadata("pipe")
                    .map(|metadata| metadata.file_type),
                directory
                    .symlink_metadata("pipe")
                    .map(|metadata| metadata.file_type),
                listed
            ),
            (
                Ok(FileType::Unknown),
                Ok(FileType::Unknown),
                vec![FileType::Unknown]
            ),
            "a FIFO looked up through Directory::metadata and symlink_metadata, and listed \
             through read_dir, must answer an unknown type"
        );
    }

    // @behavior FS-005
    #[test]
    fn a_directory_serves_the_files_of_its_host_directory() {
        let host = HostDir::new("serves");
        fs::write(host.0.join("data.csv"), "a,b").expect("the host file must be written");

        let directory = Directory::new(&host.0).expect("the host directory must open");
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
        let read = file.read_at(&mut buf, 0).expect("the host file must read");
        assert_eq!(
            &buf[..read],
            b"a,b",
            "a host file read through a Directory's open must read back its content"
        );
    }

    #[test]
    fn an_open_for_neither_reading_nor_writing_still_opens() {
        let host = HostDir::new("lookup");
        fs::write(host.0.join("seen"), "x").expect("the host file must be written");
        let directory = Directory::new(&host.0).expect("the host directory must open");
        assert!(directory.open("seen", &OpenOptions::default()).is_ok());
    }

    // @behavior FS-006
    #[test]
    fn a_directory_writes_into_its_host_directory() {
        let host = HostDir::new("writes");
        let directory = Directory::new(&host.0).expect("the host directory must open");
        directory
            .open(
                "out.txt",
                &OpenOptions {
                    write: true,
                    create: true,
                    ..OpenOptions::default()
                },
            )
            .expect("a new file must be created")
            .write_at(b"written", 0)
            .expect("the write must land");
        assert_eq!(
            fs::read(host.0.join("out.txt")).expect("the host file must exist"),
            b"written",
            "a file created through a Directory's open must land in the host directory"
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
        let directory = Directory::new(&host.0).expect("the host directory must open");
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
                file.append(b"y").err(),
                fs::read(host.0.join("f")).expect("the host file must read")
            ),
            (Some(FilesystemError::Unsupported), b"x".to_vec()),
            "an append through a Directory file on Windows must be refused as unsupported and \
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
        let directory = Directory::new(&host.0).expect("the host directory must open");
        assert!(
            matches!(
                directory.open(r"d\f", &OpenOptions::default()),
                Err(FilesystemError::InvalidInput)
            ),
            "a path holding a backslash opened through a Directory on Windows must be refused as \
             invalid input"
        );
    }

    // The host answers ENAMETOOLONG, which only a mapping naming it keeps
    // from reaching the guest as an EIO.
    // @behavior FS-065
    #[test]
    fn a_directory_answers_a_host_failure_as_the_error_naming_it() {
        let host = HostDir::new("names");
        let directory = Directory::new(&host.0).expect("the host directory must open");
        let created = directory.open(
            &"x".repeat(300),
            &OpenOptions {
                write: true,
                create: true,
                ..OpenOptions::default()
            },
        );
        assert_eq!(
            created.map(drop),
            Err(FilesystemError::InvalidFilename),
            "a name past the host's limit created through Directory::open must fail as \
             InvalidFilename, not as an unnamed failure"
        );
    }

    // Which offset a host refuses depends on its filesystem: tmpfs, XFS and
    // overlay keep files up to `i64::MAX`. The host's own EFBIG, 27 on Linux
    // and macOS alike, is what the Directory must name.
    // @behavior FS-068
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_directory_answers_a_host_file_too_large_as_too_large() {
        assert_eq!(
            to_error(io::Error::from_raw_os_error(27)),
            FilesystemError::FileTooLarge,
            "a host EFBIG through Directory's error mapping must fail as FileTooLarge, \
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
        let directory = Directory::new(&host.0).expect("the host directory must open");
        assert_eq!(
            directory.read_dir("").map(drop),
            Err(FilesystemError::IllegalSequence),
            "a host name that is not UTF-8 listed through Directory::read_dir must fail as \
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
        let directory = Directory::new(&host.0).expect("the host directory must open");
        let creates = OpenOptions {
            read: true,
            create: true,
            ..OpenOptions::default()
        };
        assert_eq!(
            (
                directory.open("note", &creates).map(drop),
                directory.open("new", &creates).map(drop),
                host.0.join("new").exists()
            ),
            (Ok(()), Ok(()), true),
            "a Directory::open that may create, of a read-only host file already there, must open \
             it, and of a missing one must create it"
        );
    }

    // The link's target is a readable file, so only the refusal of the
    // link itself keeps the open from reaching it.
    // @behavior FS-047
    #[cfg(unix)]
    #[test]
    fn a_directory_opens_no_link_an_open_asks_not_to_follow() {
        let host = HostDir::new("nofollow");
        fs::write(host.0.join("target"), "x").expect("the host file must be written");
        std::os::unix::fs::symlink("target", host.0.join("link")).expect("the link must be made");
        let directory = Directory::new(&host.0).expect("the host directory must open");
        let open = |nofollow| {
            directory
                .open(
                    "link",
                    &OpenOptions {
                        read: true,
                        nofollow,
                        ..OpenOptions::default()
                    },
                )
                .map(drop)
        };
        assert_eq!(
            (open(true), open(false)),
            (Err(FilesystemError::FilesystemLoop), Ok(())),
            "a host link opened through a Directory must be refused when the open asks to follow \
             none, and reached when it does not"
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
        let directory = Directory::new(&host.0).expect("the host directory must open");
        let listed = directory
            .read_dir("")
            .expect("the host directory must list")
            .into_iter()
            .find(|entry| entry.name == "link")
            .map(|entry| entry.file_type);
        let shape = |metadata: Metadata| (metadata.file_type, metadata.len);
        assert_eq!(
            (
                directory.symlink_metadata("link").map(shape),
                listed,
                directory.metadata("link").map(shape)
            ),
            (
                Ok((FileType::Symlink, 0)),
                Some(FileType::Symlink),
                Ok((FileType::File, 5))
            ),
            "a host link looked up through Directory::symlink_metadata and listed through \
             Directory::read_dir must report a link with no size, and Directory::metadata must \
             report the file it points to"
        );
    }

    // @behavior FS-028
    #[test]
    fn a_directory_refuses_to_remove_a_directory_as_a_file() {
        let host = HostDir::new("unlink-dir");
        fs::create_dir(host.0.join("sub")).expect("the host directory must be created");
        let directory = Directory::new(&host.0).expect("the host directory must open");
        assert_eq!(
            directory.remove_file("sub"),
            Err(FilesystemError::IsADirectory),
            "a host directory removed through Directory::remove_file must answer IsADirectory"
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
        fs::write(host.0.join("inside"), "guest").expect("the inside file must be written");
        std::os::unix::fs::symlink("inside", host.0.join("alias"))
            .expect("the inside link must be created");

        let directory = Directory::new(&host.0).expect("the host directory must open");
        assert!(
            directory
                .open(
                    "alias",
                    &OpenOptions {
                        read: true,
                        ..OpenOptions::default()
                    }
                )
                .is_ok(),
            "a link that stays inside the host directory must still open through a Directory's open"
        );
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
            "a link out of the host directory opened through a Directory's open must be refused"
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
        let directory = Directory::new(&host.0).expect("the host directory must open");
        let reported = directory.metadata("dir").expect("the directory must stand");
        assert_eq!(
            (reported.dev, reported.ino),
            (expected.dev(), expected.ino()),
            "a host directory's metadata through Directory::metadata must carry the device and \
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
        use std::sync::Arc;

        let host = HostDir::new("append");
        fs::write(host.0.join("log.txt"), "").expect("the host file must be written");
        let directory = Arc::new(Directory::new(&host.0).expect("the host directory must open"));

        let writers: Vec<_> = [b"a", b"b"]
            .into_iter()
            .map(|byte| {
                let directory = Arc::clone(&directory);
                std::thread::spawn(move || {
                    let options = OpenOptions {
                        write: true,
                        ..OpenOptions::default()
                    };
                    let mut file = directory
                        .open("log.txt", &options)
                        .expect("the host file must open to write");
                    for _ in 0..10_000 {
                        file.append(byte).expect("the append must succeed");
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
            "two handles appending to one host file through a Directory's File::append must each \
             keep every byte"
        );
    }

    // ext4 hands a removed directory's inode number to the next one made,
    // so the number staying unused while held is what the Hold keeps; a
    // host that never reuses numbers has nothing to witness.
    // @behavior FS-086
    #[cfg(target_os = "linux")]
    #[test]
    fn a_directory_held_keeps_its_inode_number_from_the_next_one_made() {
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
        if ino("dir") != first {
            eprintln!("the host does not reuse inode numbers; nothing to witness");
            return;
        }
        let directory = Directory::new(&host.0).expect("the host directory must open");
        let hold = directory
            .hold_directory("dir")
            .expect("the directory must be held");
        fs::remove_dir(host.0.join("dir")).expect("the host directory must go");
        fs::create_dir(host.0.join("dir")).expect("the host directory must be made");
        let made = ino("dir");
        drop(hold);
        assert_ne!(
            made, first,
            "a directory made where Directory::hold_directory holds a removed one must take \
             another inode number on a host reusing them"
        );
    }

    // @behavior FS-081
    #[test]
    fn a_directory_spells_names_it_matches_exactly_as_given() {
        let host = HostDir::new("spelled");
        fs::create_dir(host.0.join("dir")).expect("the host directory must be made");
        fs::write(host.0.join("dir/file"), "x").expect("the host file must be written");
        let directory = Directory::new(&host.0).expect("the host directory must open");
        assert_eq!(
            (
                directory.stored_path("dir/file"),
                directory.stored_path("dir/new/name")
            ),
            (Ok("dir/file".to_string()), Ok("dir/new/name".to_string())),
            "a path asked of Directory::stored_path in the spelling its host stores, and one \
             whose tail is missing, must come back as given"
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
        let directory = Directory::new(&host.0).expect("the host directory must open");
        assert_eq!(
            (
                directory.stored_path("PRIVATE/CAFE\u{301}"),
                directory.stored_path("Private/missing")
            ),
            (
                Ok("private/caf\u{e9}".to_string()),
                Ok("private/missing".to_string())
            ),
            "a path asked of Directory::stored_path in another case and Unicode form on a host \
             folding both must come back as the host stores each name it holds"
        );
    }
}
