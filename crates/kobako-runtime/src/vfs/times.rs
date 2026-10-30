//! What the guest sees of the times a Filesystem reports.
//!
//! A Filesystem records the host's real time whatever the invocation
//! writing to it runs under, so one shared across invocations keeps one
//! record. Each read is shown under its own invocation's Profile: a
//! hermetic one sees every time at the Unix epoch, where its wall clock
//! stands, so a time a write left behind is no clock it can read.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::filesystem::{FileOperations, FilesystemError, Metadata};
use crate::profile::Profile;

/// `metadata` as an invocation running under `profile` sees it.
pub(crate) fn mask_times(metadata: Metadata, profile: Profile) -> Metadata {
    match profile {
        Profile::Permissive => metadata,
        Profile::Hermetic => {
            let epoch = |time: Option<SystemTime>| time.map(|_| UNIX_EPOCH);
            metadata.with_times(
                epoch(metadata.accessed),
                epoch(metadata.modified),
                epoch(metadata.changed),
            )
        }
    }
}

/// `file` as an invocation running under `profile` sees it.
pub(crate) fn mask_file_times(
    file: Box<dyn FileOperations>,
    profile: Profile,
) -> Box<dyn FileOperations> {
    match profile {
        Profile::Permissive => file,
        Profile::Hermetic => Box::new(Hermetic(file)),
    }
}

/// A file read by a hermetic invocation.
struct Hermetic(Box<dyn FileOperations>);

impl FileOperations for Hermetic {
    fn read(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        self.0.read(buf, offset)
    }

    fn write(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        self.0.write(buf, offset)
    }

    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        self.0.append(buf)
    }

    fn getattr(&self) -> Result<Metadata, FilesystemError> {
        Ok(mask_times(self.0.getattr()?, Profile::Hermetic))
    }

    fn truncate(&mut self, len: u64) -> Result<(), FilesystemError> {
        self.0.truncate(len)
    }

    fn fsync(&mut self) -> Result<(), FilesystemError> {
        self.0.fsync()
    }

    fn release(self: Box<Self>) -> Result<(), FilesystemError> {
        self.0.release()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::filesystem::{Access, InodeOperations, Tmpfs};
    use crate::mount::Mount;
    use crate::vfs::{self, FileDescription, Filesystem, OpenHow, Path};

    fn path_over(tmpfs: Arc<dyn InodeOperations>, profile: Profile) -> Path {
        let mount = Mount::new("/data", Filesystem::new(tmpfs), Access::ReadWrite)
            .expect("/data is a guest path");
        Path::root(mount, profile)
    }

    /// Write `name` beneath `start`, answering the file still open.
    fn write(start: &Path, name: &str) -> Box<dyn FileOperations> {
        let how = OpenHow {
            write: true,
            create: true,
            ..OpenHow::default()
        };
        let Ok(FileDescription::File(mut file)) = vfs::do_file_open(start, name, &how, true) else {
            panic!("{name} must open as a file");
        };
        file.write(b"x", 0).expect("the file must take a write");
        file
    }

    fn times(metadata: &Metadata) -> [Option<SystemTime>; 3] {
        [metadata.accessed, metadata.modified, metadata.changed]
    }

    // @behavior RT-156
    #[test]
    fn the_weaker_posture_sees_the_times_a_filesystem_keeps() {
        let start = path_over(Arc::new(Tmpfs::new()), Profile::Permissive);
        let before = SystemTime::now();
        write(&start, "file");
        let modified = vfs::vfs_fstatat(&start, "file", true)
            .expect("the file must stand")
            .modified
            .expect("a Tmpfs keeps a modification time");
        assert!(
            modified >= before,
            "vfs::vfs_fstatat of a file written beneath a Path on the weaker posture must report a \
             modification time no earlier than the write"
        );
    }

    // A Tmpfs records the host's real time, so only the posture's view
    // turns it into the epoch.
    // @behavior RT-157
    #[test]
    fn the_strongest_posture_sees_every_time_at_the_unix_epoch() {
        let start = path_over(Arc::new(Tmpfs::new()), Profile::Hermetic);
        let file = write(&start, "file");
        let epoch = [Some(UNIX_EPOCH); 3];
        assert_eq!(
            (
                times(&vfs::vfs_fstatat(&start, "file", true).expect("the file must stand")),
                times(&file.getattr().expect("the open file must report")),
            ),
            (epoch, epoch),
            "vfs::vfs_fstatat of a written file, and FileOperations::getattr of it opened through vfs::do_file_open, \
             beneath a Path on the strongest posture must report each time as the Unix epoch"
        );
    }

    // @behavior RT-158
    #[test]
    fn a_time_another_posture_wrote_reaches_the_strongest_posture_as_the_epoch() {
        let tmpfs: Arc<dyn InodeOperations> = Arc::new(Tmpfs::new());
        write(&path_over(tmpfs.clone(), Profile::Permissive), "file");
        let hermetic = path_over(tmpfs, Profile::Hermetic);
        assert_eq!(
            times(&vfs::vfs_fstatat(&hermetic, "file", true).expect("the file must stand")),
            [Some(UNIX_EPOCH); 3],
            "vfs::vfs_fstatat beneath a Path on the strongest posture, of a file the weaker posture \
             wrote into the same Tmpfs, must report each time as the Unix epoch"
        );
    }
}
