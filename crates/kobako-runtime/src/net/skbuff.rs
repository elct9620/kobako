//! The bytes a stream holds, as Linux's `sk_buff_head` chains a socket's
//! buffers: each push is a buffer of its own, sized to what it holds, freed
//! once wholly read.

use std::collections::VecDeque;

/// A stream's queued bytes, oldest first.
#[derive(Default)]
pub(crate) struct SkBuffHead {
    buffers: VecDeque<Vec<u8>>,
    /// How much of the oldest buffer was read already.
    offset: usize,
    len: usize,
}

impl SkBuffHead {
    /// The bytes it holds unread.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Queue `bytes` as one buffer of their own.
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.push_buffer(bytes.to_vec());
    }

    /// Queue `buffer` as it is, as `__skb_queue_tail` links an `sk_buff`.
    pub(crate) fn push_buffer(&mut self, buffer: Vec<u8>) {
        if buffer.is_empty() {
            return;
        }
        self.len += buffer.len();
        self.buffers.push_back(buffer);
    }

    /// The oldest unread bytes, as one slice.
    pub(crate) fn front(&self) -> &[u8] {
        self.buffers
            .front()
            .map_or(&[], |buffer| &buffer[self.offset..])
    }

    /// Mark `n` of the oldest bytes read, freeing each buffer wholly read.
    pub(crate) fn advance(&mut self, mut n: usize) {
        n = n.min(self.len);
        self.len -= n;
        while n > 0 {
            let Some(buffer) = self.buffers.front() else {
                break;
            };
            let left = buffer.len() - self.offset;
            if n < left {
                self.offset += n;
                return;
            }
            n -= left;
            self.buffers.pop_front();
            self.offset = 0;
        }
    }

    /// Copy the oldest bytes into `buf`, as many as fit, and free what was
    /// wholly read; answers how many it copied.
    pub(crate) fn read_into(&mut self, buf: &mut [u8]) -> usize {
        let mut copied = 0;
        while copied < buf.len() && !self.is_empty() {
            let front = self.front();
            let n = front.len().min(buf.len() - copied);
            buf[copied..copied + n].copy_from_slice(&front[..n]);
            copied += n;
            self.advance(n);
        }
        copied
    }

    /// Free every buffer, as `__skb_queue_purge` does.
    pub(crate) fn purge(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_queue_answers_its_bytes_in_order_and_frees_each_buffer_wholly_read() {
        let mut queue = SkBuffHead::default();
        queue.push(b"hello");
        queue.push(b"world");
        let mut buf = [0; 7];
        let copied = queue.read_into(&mut buf);
        assert_eq!(
            (
                &buf[..copied],
                queue.len(),
                queue.buffers.len(),
                queue.front()
            ),
            (&b"hellowo"[..], 3, 1, &b"rld"[..]),
            "a queue of two 5-byte buffers read 7 bytes into must answer them in order, \
             keep only the buffer still partly unread, and answer its rest first"
        );
    }
}
