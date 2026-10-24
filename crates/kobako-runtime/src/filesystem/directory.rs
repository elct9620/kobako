//! `Directory`: a Filesystem over a host directory, from which nothing
//! outside it is reachable.
//!
//! Every path resolves through `cap_std::fs::Dir`, which refuses a `..`
//! or a symbolic link that would lead out of the directory, so the
//! confinement holds whatever path a caller hands over.

use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions as CapOpenOptions};

use super::{DirEntry, File, FileType, Filesystem, FilesystemError, Metadata, OpenOptions, Rename};

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
    } else {
        FileType::File
    }
}

/// What a host entry is, a file's size alone counted: a link reports no
/// size, so nothing of where it points reaches the guest.
fn to_metadata(metadata: cap_std::fs::Metadata) -> Metadata {
    let file_type = to_file_type(metadata.file_type());
    Metadata {
        file_type,
        len: if file_type == FileType::File {
            metadata.len()
        } else {
            0
        },
    }
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
        // The host could swap a link in between; the host's to rule out.
        if options.nofollow
            && self
                .dir
                .symlink_metadata(to_host_path(path)?)
                .is_ok_and(|metadata| metadata.is_symlink())
        {
            return Err(FilesystemError::FilesystemLoop);
        }
        let file = self
            .dir
            .open_with(to_host_path(path)?, &open)
            .map_err(to_error)?;
        if file.metadata().map_err(to_error)?.is_dir() {
            return Err(FilesystemError::IsADirectory);
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

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        let metadata = self.file.metadata().map_err(to_error)?;
        Ok(Metadata {
            file_type: FileType::File,
            len: metadata.len(),
        })
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
        assert_eq!(
            (
                directory.symlink_metadata("link"),
                listed,
                directory.metadata("link")
            ),
            (
                Ok(Metadata {
                    file_type: FileType::Symlink,
                    len: 0
                }),
                Some(FileType::Symlink),
                Ok(Metadata {
                    file_type: FileType::File,
                    len: 5
                })
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
}
