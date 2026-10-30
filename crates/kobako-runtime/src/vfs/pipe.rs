//! The in-memory pipes an invocation's standard streams run over, as Linux's
//! pipefs holds anonymous pipes. Any ABI a Driver encodes reads the Preamble
//! and writes a Channel through them, so none brings its own, and a pipe's two
//! ends are open files like any other.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use super::dentry::Dentry;
use super::file::File;
use super::inode::Inode;
use super::openat2::OpenHow;
use super::path::Path;
use crate::filesystem::{
    Access, FileOperations, FileType, FilesystemError, InodeOperations, Kiocb, Metadata,
    OpenOptions,
};
use crate::mount::Mount;
use crate::profile::Profile;
use crate::vfs::Filesystem;

/// The one Filesystem every pipe stands on, as Linux's `pipe_mnt`. No guest
/// path reaches it: a pipe is only ever open.
static PIPE_MNT: LazyLock<Path> = LazyLock::new(|| {
    Path::root(
        Mount::new("/", Filesystem::new(Arc::new(PipeFs)), Access::ReadWrite)
            .expect("/ is a guest path"),
        Profile::Permissive,
    )
});

/// Each pipe's name on pipefs, which no guest ever reads.
static PIPE_NAME: AtomicU64 = AtomicU64::new(0);

/// A pipe reports what it is and no length, as `get_pipe_inode` makes one.
struct PipeFs;

impl InodeOperations for PipeFs {
    fn getattr(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        Ok(Metadata::new(FileType::Unknown, 0))
    }

    fn open(
        &self,
        _dentry: &Dentry,
        _options: &OpenOptions,
    ) -> Option<Result<Box<dyn FileOperations>, FilesystemError>> {
        None
    }
}

/// What a pipe holds, as Linux's `pipe_inode_info`: what was written and not
/// yet read, up to a capacity.
pub struct PipeInodeInfo {
    state: Mutex<PipeState>,
    capacity: usize,
}

struct PipeState {
    bytes: Vec<u8>,
    read: usize,
}

impl PipeInodeInfo {
    /// An empty pipe holding at most `capacity` bytes.
    pub fn new(capacity: usize) -> Arc<Self> {
        Self::holding(Vec::new(), capacity)
    }

    /// A pipe already holding `bytes`, as the Preamble's stream does, with no
    /// room for more.
    pub fn filled(bytes: Vec<u8>) -> Arc<Self> {
        let capacity = bytes.len();
        Self::holding(bytes, capacity)
    }

    fn holding(bytes: Vec<u8>, capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(PipeState { bytes, read: 0 }),
            capacity,
        })
    }

    /// Everything written so far, as a Channel's capture reads it back.
    pub fn contents(&self) -> Vec<u8> {
        self.state().bytes.clone()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, PipeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What answers for either end of a pipe, as Linux's `pipeanon_fops`.
struct AnonPipe(Arc<PipeInodeInfo>);

impl FileOperations for AnonPipe {
    /// A pipe has no offsets, so a read takes what comes next. A read of
    /// nothing from a pipe still holding bytes answers `Interrupted`, as a
    /// blocking read given nothing to fill returns before it would wait; one
    /// from a drained pipe reads nothing, as a closed writer's pipe does.
    fn read_iter(&mut self, _iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        let mut state = self.0.state();
        let start = state.read;
        let rest = &state.bytes[start..];
        if buf.is_empty() && !rest.is_empty() {
            return Err(FilesystemError::Interrupted);
        }
        let take = rest.len().min(buf.len());
        buf[..take].copy_from_slice(&rest[..take]);
        state.read += take;
        Ok(take)
    }

    /// A pipe has no offsets, so a write lands after the last. A write past
    /// the capacity keeps what fit and answers a failure, which is how a
    /// capture one past its limit learns it was truncated.
    fn write_iter(&mut self, _iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
        let mut state = self.0.state();
        let take = buf.len().min(self.0.capacity - state.bytes.len());
        state.bytes.extend_from_slice(&buf[..take]);
        if take < buf.len() {
            return Err(FilesystemError::Other("the pipe is full".to_string()));
        }
        Ok(take)
    }
}

/// The read and the write end of `pipe`, as Linux's `create_pipe_files` opens
/// both on one inode. Neither takes a position.
pub fn create_pipe_files(pipe: &Arc<PipeInodeInfo>) -> (File, File) {
    let name = PIPE_NAME.fetch_add(1, Ordering::Relaxed).to_string();
    let dentries = PIPE_MNT.mount().filesystem().dentries();
    let dentry = dentries.child(PIPE_MNT.dentry(), &name);
    let found = Metadata::new(FileType::Unknown, 0);
    dentry.add(Some(Inode::new(&found, Arc::new(PipeFs))));
    let end = |how: OpenHow| {
        let mut file = File::new(
            PIPE_MNT.with_dentry(dentry.clone()),
            Box::new(AnonPipe(pipe.clone())),
            &how,
        );
        file.stream_open();
        file
    };
    (
        end(OpenHow {
            read: true,
            ..OpenHow::default()
        }),
        end(OpenHow {
            write: true,
            ..OpenHow::default()
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_reads_in_order_then_nothing() {
        let (mut reader, _) = create_pipe_files(&PipeInodeInfo::filled(b"abc".to_vec()));
        let mut buf = [0; 2];
        assert_eq!(
            (
                reader.read(&mut buf),
                buf,
                reader.read(&mut buf),
                reader.read(&mut buf)
            ),
            (Ok(2), *b"ab", Ok(1), Ok(0)),
            "File::read of a pipe's read end holding three bytes in steps of two must take them \
             in order, then nothing"
        );
    }

    #[test]
    fn a_read_of_nothing_is_interrupted_only_while_bytes_remain() {
        let (mut holding, _) = create_pipe_files(&PipeInodeInfo::filled(b"a".to_vec()));
        let (mut drained, _) = create_pipe_files(&PipeInodeInfo::filled(Vec::new()));
        assert_eq!(
            (holding.read(&mut []), drained.read(&mut [])),
            (Err(FilesystemError::Interrupted), Ok(0)),
            "File::read of a pipe into no room must answer Interrupted while bytes remain and \
             nothing once drained"
        );
    }

    #[test]
    fn a_writer_keeps_what_fits_and_fails_past_its_capacity() {
        let pipe = PipeInodeInfo::new(3);
        let (_, mut writer) = create_pipe_files(&pipe);
        assert_eq!(
            (
                writer.write(b"ab").ok(),
                writer.write(b"cd").is_err(),
                pipe.contents()
            ),
            (Some(2), true, b"abc".to_vec()),
            "File::write past a pipe's capacity must keep what fit and fail, and \
             PipeInodeInfo::contents must read what was written"
        );
    }

    #[test]
    fn a_pipe_takes_no_position() {
        let (reader, writer) = create_pipe_files(&PipeInodeInfo::new(1));
        for end in [reader, writer] {
            assert!(
                end.is_stream() && !end.mode().contains(super::super::FMode::LSEEK),
                "create_pipe_files must open each end as a stream with no position"
            );
        }
    }
}
