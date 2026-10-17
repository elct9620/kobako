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

use super::{DirEntry, File, Filesystem, FilesystemError, Kind, Metadata, OpenOptions};

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

/// The root of a Filesystem is the empty path; the directory itself to
/// `cap_std` is `.`.
fn host(path: &str) -> &str {
    if path.is_empty() {
        "."
    } else {
        path
    }
}

fn error(err: io::Error) -> FilesystemError {
    match err.kind() {
        io::ErrorKind::NotFound => FilesystemError::NotFound,
        io::ErrorKind::AlreadyExists => FilesystemError::AlreadyExists,
        io::ErrorKind::NotADirectory => FilesystemError::NotADirectory,
        io::ErrorKind::IsADirectory => FilesystemError::IsADirectory,
        io::ErrorKind::DirectoryNotEmpty => FilesystemError::DirectoryNotEmpty,
        io::ErrorKind::PermissionDenied => FilesystemError::PermissionDenied,
        io::ErrorKind::ReadOnlyFilesystem => FilesystemError::ReadOnly,
        io::ErrorKind::InvalidInput => FilesystemError::InvalidInput,
        io::ErrorKind::Unsupported => FilesystemError::Unsupported,
        _ => FilesystemError::Other(err.to_string()),
    }
}

fn kind(is_dir: bool) -> Kind {
    if is_dir {
        Kind::Directory
    } else {
        Kind::File
    }
}

impl Filesystem for Directory {
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        let creates = options.create || options.create_new || options.truncate;
        let mut open = CapOpenOptions::new();
        open.read(options.read)
            // The host refuses to create or truncate a file it is not
            // opening for writing; the runtime decides what is written.
            .write(options.write || creates)
            .create(options.create)
            .create_new(options.create_new)
            .truncate(options.truncate);
        let file = self.dir.open_with(host(path), &open).map_err(error)?;
        if file.metadata().map_err(error)?.is_dir() {
            return Err(FilesystemError::IsADirectory);
        }
        Ok(Box::new(DirectoryFile {
            file: file.into_std(),
        }))
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        let metadata = self.dir.metadata(host(path)).map_err(error)?;
        Ok(Metadata {
            kind: kind(metadata.is_dir()),
            len: if metadata.is_dir() { 0 } else { metadata.len() },
        })
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        self.dir
            .read_dir(host(path))
            .map_err(error)?
            .map(|entry| {
                let entry = entry.map_err(error)?;
                let name = entry.file_name().into_string().map_err(|name| {
                    FilesystemError::Other(format!("a host entry name is not UTF-8: {name:?}"))
                })?;
                let is_dir = entry.file_type().map_err(error)?.is_dir();
                Ok(DirEntry {
                    name,
                    kind: kind(is_dir),
                })
            })
            .collect()
    }

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.dir.create_dir(host(path)).map_err(error)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.dir.remove_dir(host(path)).map_err(error)
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        self.dir.remove_file(host(path)).map_err(error)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), FilesystemError> {
        self.dir
            .rename(host(from), &self.dir, host(to))
            .map_err(error)
    }
}

/// An open host file. The runtime names every offset, so each access seeks
/// first.
struct DirectoryFile {
    file: std::fs::File,
}

impl File for DirectoryFile {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        self.file.seek(SeekFrom::Start(offset)).map_err(error)?;
        self.file.read(buf).map_err(error)
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        self.file.seek(SeekFrom::Start(offset)).map_err(error)?;
        self.file.write(buf).map_err(error)
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        let metadata = self.file.metadata().map_err(error)?;
        Ok(Metadata {
            kind: Kind::File,
            len: metadata.len(),
        })
    }

    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
        self.file.set_len(len).map_err(error)
    }

    fn sync(&mut self) -> Result<(), FilesystemError> {
        self.file.sync_all().map_err(error)
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
