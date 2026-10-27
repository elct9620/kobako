//! What the guest sees of the times a Filesystem reports.
//!
//! A Filesystem records the host's real time whatever the invocation
//! writing to it runs under, so one shared across invocations keeps one
//! record. Each read is shown under its own invocation's Profile: a
//! hermetic one sees every time at the Unix epoch, where its wall clock
//! stands, so a time a write left behind is no clock it can read.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::filesystem::{File, FilesystemError, Metadata};
use crate::profile::Profile;

/// `metadata` as an invocation running under `profile` sees it.
pub(crate) fn seen(metadata: Metadata, profile: Profile) -> Metadata {
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
pub(crate) fn seen_file(file: Box<dyn File>, profile: Profile) -> Box<dyn File> {
    match profile {
        Profile::Permissive => file,
        Profile::Hermetic => Box::new(Hermetic(file)),
    }
}

/// A file read by a hermetic invocation.
struct Hermetic(Box<dyn File>);

impl File for Hermetic {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        self.0.read_at(buf, offset)
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        self.0.write_at(buf, offset)
    }

    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        self.0.append(buf)
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        Ok(seen(self.0.metadata()?, Profile::Hermetic))
    }

    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
        self.0.set_len(len)
    }

    fn sync(&mut self) -> Result<(), FilesystemError> {
        self.0.sync()
    }

    fn close(self: Box<Self>) -> Result<(), FilesystemError> {
        self.0.close()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::filesystem::{Access, Filesystem, Memory};
    use crate::mount::Mount;
    use crate::vfs::{self, OpenHow, Opened, Path, SuperBlock};

    fn path_over(memory: Arc<dyn Filesystem>, profile: Profile) -> Path {
        let mount = Mount::new("/data", SuperBlock::new(memory), Access::ReadWrite)
            .expect("/data is a guest path");
        Path::root(mount, profile)
    }

    /// Write `name` beneath `start`, answering the file still open.
    fn write(start: &Path, name: &str) -> Box<dyn File> {
        let how = OpenHow {
            write: true,
            create: true,
            ..OpenHow::default()
        };
        let Ok(Opened::File(mut file)) = vfs::open(start, name, &how, true) else {
            panic!("{name} must open as a file");
        };
        file.write_at(b"x", 0).expect("the file must take a write");
        file
    }

    fn times(metadata: &Metadata) -> [Option<SystemTime>; 3] {
        [metadata.accessed, metadata.modified, metadata.changed]
    }

    // @behavior RT-156
    #[test]
    fn the_weaker_posture_sees_the_times_a_filesystem_keeps() {
        let start = path_over(Arc::new(Memory::new()), Profile::Permissive);
        let before = SystemTime::now();
        write(&start, "file");
        let modified = vfs::metadata(&start, "file")
            .expect("the file must stand")
            .modified
            .expect("a Memory keeps a modification time");
        assert!(
            modified >= before,
            "vfs::metadata of a file written beneath a Path on the weaker posture must report a \
             modification time no earlier than the write"
        );
    }

    // A Memory records the host's real time, so only the posture's view
    // turns it into the epoch.
    // @behavior RT-157
    #[test]
    fn the_strongest_posture_sees_every_time_at_the_unix_epoch() {
        let start = path_over(Arc::new(Memory::new()), Profile::Hermetic);
        let file = write(&start, "file");
        let epoch = [Some(UNIX_EPOCH); 3];
        assert_eq!(
            (
                times(&vfs::metadata(&start, "file").expect("the file must stand")),
                times(&file.metadata().expect("the open file must report")),
            ),
            (epoch, epoch),
            "vfs::metadata of a written file, and File::metadata of it opened through vfs::open, \
             beneath a Path on the strongest posture must report each time as the Unix epoch"
        );
    }

    // @behavior RT-158
    #[test]
    fn a_time_another_posture_wrote_reaches_the_strongest_posture_as_the_epoch() {
        let memory: Arc<dyn Filesystem> = Arc::new(Memory::new());
        write(&path_over(memory.clone(), Profile::Permissive), "file");
        let hermetic = path_over(memory, Profile::Hermetic);
        assert_eq!(
            times(&vfs::metadata(&hermetic, "file").expect("the file must stand")),
            [Some(UNIX_EPOCH); 3],
            "vfs::metadata beneath a Path on the strongest posture, of a file the weaker posture \
             wrote into the same Memory, must report each time as the Unix epoch"
        );
    }
}
