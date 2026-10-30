//! The in-memory pipes an invocation's standard streams run over, as Linux's
//! `pipefifo_fops` answers a pipe's two ends. Any ABI a Driver encodes reads
//! the Preamble and writes a Channel through them, so none brings its own.

use std::sync::{Arc, Mutex, PoisonError};

use crate::filesystem::{FileOperations, FilesystemError};

/// The read end of a pipe holding fixed bytes, as the Preamble's stream is.
pub struct PipeReader {
    bytes: Vec<u8>,
    read: usize,
}

impl PipeReader {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, read: 0 }
    }
}

impl FileOperations for PipeReader {
    /// A pipe has no offsets, so a read takes what comes next. A read of
    /// nothing from a pipe still holding bytes answers `Interrupted`, as a
    /// blocking read given nothing to fill returns before it would wait; one
    /// from a drained pipe reads nothing, as a closed writer's pipe does.
    fn read(&mut self, buf: &mut [u8], _offset: u64) -> Result<usize, FilesystemError> {
        let rest = &self.bytes[self.read..];
        if buf.is_empty() && !rest.is_empty() {
            return Err(FilesystemError::Interrupted);
        }
        let take = rest.len().min(buf.len());
        buf[..take].copy_from_slice(&rest[..take]);
        self.read += take;
        Ok(take)
    }
}

/// The write end of a pipe holding at most a capacity's bytes, each clone
/// writing into the same buffer, as a Channel's capture is one.
#[derive(Clone)]
pub struct PipeWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
    capacity: usize,
}

impl PipeWriter {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Arc::default(),
            capacity,
        }
    }

    /// What was written so far.
    pub fn contents(&self) -> Vec<u8> {
        self.buffer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl FileOperations for PipeWriter {
    /// A pipe has no offsets, so a write lands after the last. A write past
    /// the capacity keeps what fit and answers a failure, which is how a
    /// capture one past its limit learns it was truncated.
    fn write(&mut self, buf: &[u8], _offset: u64) -> Result<usize, FilesystemError> {
        let mut buffer = self.buffer.lock().unwrap_or_else(PoisonError::into_inner);
        let take = buf.len().min(self.capacity - buffer.len());
        buffer.extend_from_slice(&buf[..take]);
        if take < buf.len() {
            return Err(FilesystemError::Other("the pipe is full".to_string()));
        }
        Ok(take)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_reads_in_order_then_nothing() {
        let mut reader = PipeReader::new(b"abc".to_vec());
        let mut buf = [0; 2];
        assert_eq!(
            (
                reader.read(&mut buf, 0),
                buf,
                reader.read(&mut buf, 0),
                reader.read(&mut buf, 0)
            ),
            (Ok(2), *b"ab", Ok(1), Ok(0)),
            "PipeReader::read of three bytes in steps of two must take them in order, then nothing"
        );
    }

    #[test]
    fn a_read_of_nothing_is_interrupted_only_while_bytes_remain() {
        let mut holding = PipeReader::new(b"a".to_vec());
        let mut drained = PipeReader::new(Vec::new());
        assert_eq!(
            (holding.read(&mut [], 0), drained.read(&mut [], 0)),
            (Err(FilesystemError::Interrupted), Ok(0)),
            "PipeReader::read into no room must answer Interrupted while bytes remain and nothing \
             once drained"
        );
    }

    #[test]
    fn a_writer_keeps_what_fits_and_fails_past_its_capacity() {
        let mut writer = PipeWriter::new(3);
        let shared = writer.clone();
        assert_eq!(
            (
                writer.write(b"ab", 0).ok(),
                writer.write(b"cd", 0).is_err(),
                shared.contents()
            ),
            (Some(2), true, b"abc".to_vec()),
            "PipeWriter::write past its capacity must fail, keep what fit, and show it through \
             every clone"
        );
    }
}
