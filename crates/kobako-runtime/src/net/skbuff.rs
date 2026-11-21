//! The bytes a stream holds, as Linux's `sk_buff_head` chains a socket's
//! buffers: each push is a buffer of its own, sized to what it holds, freed
//! once wholly read, so what the queue costs is what it holds.

use std::collections::VecDeque;

/// What one buffer costs beside its bytes, as `skb->truesize` counts the
/// `sk_buff` itself.
const SKB_OVERHEAD: usize = std::mem::size_of::<Vec<u8>>();

/// The page a socket's memory is charged in, as `sk_mem_pages` rounds a
/// charge up to `PAGE_SIZE`.
const PAGE_SIZE: usize = 4096;

/// What `truesize` bytes of buffers hold against the Invocation's memory,
/// in whole pages, as `sk_forward_alloc` holds a socket's charge.
pub(crate) fn sk_mem_pages(truesize: usize) -> usize {
    truesize.div_ceil(PAGE_SIZE) * PAGE_SIZE
}

/// What a buffer of `len` bytes costs, as `SKB_TRUESIZE` counts one.
pub(crate) fn truesize_of(len: usize) -> usize {
    len + SKB_OVERHEAD
}

/// A stream's queued bytes, oldest first.
#[derive(Default)]
pub(crate) struct SkBuffHead {
    buffers: VecDeque<Vec<u8>>,
    /// How much of the oldest buffer was read already.
    offset: usize,
    len: usize,
    truesize: usize,
}

impl SkBuffHead {
    /// The bytes it holds unread.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// What its buffers cost, read or not, as `sk_rmem_alloc` and
    /// `sk_wmem_queued` count each buffer until it is freed.
    pub(crate) fn truesize(&self) -> usize {
        self.truesize
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
        self.truesize += truesize_of(buffer.len());
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
            self.truesize -= truesize_of(buffer.len());
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
    fn a_queue_costs_its_buffers_until_each_is_wholly_read() {
        let mut queue = SkBuffHead::default();
        queue.push(b"hello");
        queue.push(b"world");
        let mut buf = [0; 7];
        let copied = queue.read_into(&mut buf);
        assert_eq!(
            (&buf[..copied], queue.len(), queue.truesize(), queue.front()),
            (&b"hellowo"[..], 3, truesize_of(5), &b"rld"[..]),
            "a queue of two 5-byte buffers read 7 bytes into must answer them in order, \
             cost only the buffer still partly unread, and answer its rest first"
        );
    }

    #[test]
    fn a_charge_rounds_up_to_whole_pages() {
        assert_eq!(
            (
                sk_mem_pages(0),
                sk_mem_pages(1),
                sk_mem_pages(4096),
                sk_mem_pages(4097)
            ),
            (0, 4096, 4096, 8192),
            "a socket's charge must round up to whole 4 KiB pages, as sk_mem_pages does"
        );
    }
}
