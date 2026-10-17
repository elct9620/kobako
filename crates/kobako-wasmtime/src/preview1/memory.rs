//! The guest's linear memory as one preview1 call sees it: little-endian
//! fields at guest addresses. An address outside the memory, or one not
//! aligned for what it points at, traps, as preview1 requires.

use std::ops::Range;

use super::errno::{Answer, Errno, Failure};

fn trap(message: &'static str) -> Failure {
    Failure::Trap(wasmtime::Error::msg(message))
}

fn out_of_bounds() -> Failure {
    trap("pointer out of bounds of guest memory")
}

pub(crate) struct GuestMemory<'a> {
    bytes: &'a mut [u8],
}

impl<'a> GuestMemory<'a> {
    pub(crate) fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes }
    }

    fn range(&self, ptr: u32, len: u32) -> Answer<Range<usize>> {
        let start = ptr as usize;
        match start.checked_add(len as usize) {
            Some(end) if end <= self.bytes.len() => Ok(start..end),
            _ => Err(out_of_bounds()),
        }
    }

    /// Hold `ptr` to the alignment of the struct or integer it points at.
    pub(crate) fn aligned(ptr: u32, align: u32) -> Answer {
        if !ptr.is_multiple_of(align) {
            return Err(trap("pointer not aligned for what it points at"));
        }
        Ok(())
    }

    /// The address of element `index` of an array of `size`-byte elements
    /// at `base`.
    pub(crate) fn element(base: u32, index: u32, size: u32) -> Answer<u32> {
        index
            .checked_mul(size)
            .and_then(|offset| base.checked_add(offset))
            .ok_or_else(out_of_bounds)
    }

    pub(crate) fn bytes(&self, ptr: u32, len: u32) -> Answer<&[u8]> {
        let range = self.range(ptr, len)?;
        Ok(&self.bytes[range])
    }

    pub(crate) fn bytes_mut(&mut self, ptr: u32, len: u32) -> Answer<&mut [u8]> {
        let range = self.range(ptr, len)?;
        Ok(&mut self.bytes[range])
    }

    fn read<const N: usize>(&self, ptr: u32) -> Answer<[u8; N]> {
        Self::aligned(ptr, N as u32)?;
        let mut out = [0; N];
        out.copy_from_slice(self.bytes(ptr, N as u32)?);
        Ok(out)
    }

    pub(crate) fn read_u8(&self, ptr: u32) -> Answer<u8> {
        self.read::<1>(ptr).map(|b| b[0])
    }

    pub(crate) fn read_u16(&self, ptr: u32) -> Answer<u16> {
        self.read(ptr).map(u16::from_le_bytes)
    }

    pub(crate) fn read_u32(&self, ptr: u32) -> Answer<u32> {
        self.read(ptr).map(u32::from_le_bytes)
    }

    pub(crate) fn read_u64(&self, ptr: u32) -> Answer<u64> {
        self.read(ptr).map(u64::from_le_bytes)
    }

    /// Copy `bytes` into guest memory at `ptr`, which needs no alignment.
    pub(crate) fn write(&mut self, ptr: u32, bytes: &[u8]) -> Answer {
        self.bytes_mut(ptr, bytes.len() as u32)?
            .copy_from_slice(bytes);
        Ok(())
    }

    fn write_aligned<const N: usize>(&mut self, ptr: u32, bytes: [u8; N]) -> Answer {
        Self::aligned(ptr, N as u32)?;
        self.write(ptr, &bytes)
    }

    pub(crate) fn write_u8(&mut self, ptr: u32, value: u8) -> Answer {
        self.write(ptr, &[value])
    }

    pub(crate) fn write_u16(&mut self, ptr: u32, value: u16) -> Answer {
        self.write_aligned(ptr, value.to_le_bytes())
    }

    pub(crate) fn write_u32(&mut self, ptr: u32, value: u32) -> Answer {
        self.write_aligned(ptr, value.to_le_bytes())
    }

    pub(crate) fn write_u64(&mut self, ptr: u32, value: u64) -> Answer {
        self.write_aligned(ptr, value.to_le_bytes())
    }

    /// A guest string, which preview1 requires to be UTF-8.
    pub(crate) fn string(&self, ptr: u32, len: u32) -> Answer<String> {
        let bytes = self.bytes(ptr, len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| Errno::Ilseq.into())
    }

    /// The first non-empty buffer of an `iovec` / `ciovec` array, as
    /// `(ptr, len)`, or an empty one when every buffer is empty. Like
    /// wasmtime-wasi, kobako moves one buffer per call.
    pub(crate) fn first_non_empty_buffer(&self, array: u32, count: u32) -> Answer<(u32, u32)> {
        for index in 0..count {
            let entry = Self::element(array, index, 8)?;
            let ptr = self.read_u32(entry)?;
            let len = self.read_u32(Self::element(entry, 1, 4)?)?;
            if len != 0 {
                self.range(ptr, len)?;
                return Ok((ptr, len));
            }
        }
        Ok((0, 0))
    }
}
