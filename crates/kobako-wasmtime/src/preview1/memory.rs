//! The guest's linear memory as one preview1 call sees it: little-endian
//! fields at guest addresses. An address outside the memory, or one not
//! aligned for what it points at, traps, as preview1 requires.

use std::cell::Cell;
use std::ops::Range;

use super::beneath::PATH_MAX;
use super::errno::{Answer, Errno, Failure};

fn trap(message: &'static str) -> Failure {
    Failure::Trap(wasmtime::Error::msg(message))
}

fn out_of_bounds() -> Failure {
    trap("pointer out of bounds of guest memory")
}

/// The most of the guest's memory one call may have the host copy or
/// walk, as wasmtime's default hostcall fuel bounds a preview1 call; past
/// it the call answers NOMEM.
const HOSTCALL_FUEL: u64 = 128 << 20;

pub(crate) struct GuestMemory<'a> {
    bytes: &'a mut [u8],
    fuel: Cell<u64>,
}

impl<'a> GuestMemory<'a> {
    pub(crate) fn new(bytes: &'a mut [u8]) -> Self {
        Self {
            bytes,
            fuel: Cell::new(HOSTCALL_FUEL),
        }
    }

    /// Spend `count` elements of `size` bytes of this call's fuel, as
    /// wasmtime-wasi spends it before it copies an array.
    pub(crate) fn consume(&self, count: u32, size: u32) -> Answer {
        let cost = u64::from(count) * u64::from(size);
        let left = self.fuel.get().checked_sub(cost).ok_or(Errno::Nomem)?;
        self.fuel.set(left);
        Ok(())
    }

    fn range(&self, ptr: u32, len: u32) -> Answer<Range<usize>> {
        let start = ptr as usize;
        match start.checked_add(len as usize) {
            Some(end) if end <= self.bytes.len() => Ok(start..end),
            _ => Err(out_of_bounds()),
        }
    }

    /// Hold `ptr` to the alignment of the struct or integer it points at.
    pub(crate) fn check_aligned(ptr: u32, align: u32) -> Answer {
        if !ptr.is_multiple_of(align) {
            return Err(trap("pointer not aligned for what it points at"));
        }
        Ok(())
    }

    /// The address of element `index` of an array of `size`-byte elements
    /// at `base`.
    pub(crate) fn element(base: u32, index: u32, size: u32) -> Answer<u32> {
        Self::field(base, index.checked_mul(size).ok_or_else(out_of_bounds)?)
    }

    /// The address `offset` bytes past `base`, which traps rather than
    /// wrapping past the end of the address space.
    pub(crate) fn field(base: u32, offset: u32) -> Answer<u32> {
        base.checked_add(offset).ok_or_else(out_of_bounds)
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
        Self::check_aligned(ptr, N as u32)?;
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
        Self::check_aligned(ptr, N as u32)?;
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

    /// A guest path, which preview1 requires to be UTF-8, charged to the
    /// call's fuel before it is read, as wasmtime-wasi charges it. One
    /// that would not fit Linux's `PATH_MAX` with its terminating NUL is
    /// refused before it is copied, as Linux's `getname` refuses it, and
    /// one holding a NUL names nothing a host can hold, as cap-std refuses
    /// it.
    pub(crate) fn path(&self, ptr: u32, len: u32) -> Answer<String> {
        self.consume(len, 1)?;
        let bytes = self.bytes(ptr, len)?;
        if bytes.len() >= PATH_MAX {
            return Err(Errno::Nametoolong.into());
        }
        if bytes.contains(&0) {
            return Err(Errno::Inval.into());
        }
        String::from_utf8(bytes.to_vec()).map_err(|_| Errno::Ilseq.into())
    }

    /// The first non-empty buffer of an `iovec` / `ciovec` array, as
    /// `(ptr, len)`, or an empty one when every buffer is empty. Like
    /// wasmtime-wasi, kobako moves one buffer per call, and the buffer
    /// itself is bounded only once the call has found its descriptor.
    pub(crate) fn first_non_empty_buffer(&self, array: u32, count: u32) -> Answer<(u32, u32)> {
        self.consume(count, 8)?;
        for index in 0..count {
            let entry = Self::element(array, index, 8)?;
            let ptr = self.read_u32(entry)?;
            let len = self.read_u32(Self::field(entry, 4)?)?;
            if len != 0 {
                self.consume(len, 1)?;
                return Ok((ptr, len));
            }
        }
        Ok((0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Neither the array nor the buffer exists in this small memory, so the
    // budget alone, spent before anything is read, refuses each.
    // @behavior RT-096
    #[test]
    fn a_call_past_its_hostcall_fuel_answers_nomem() {
        let mut array_bytes = [0u8; 64];
        let mut buffer_bytes = [0u8; 64];
        buffer_bytes[4..8].copy_from_slice(&(128u32 << 20).wrapping_add(1).to_le_bytes());
        assert!(
            matches!(
                (
                    GuestMemory::new(&mut array_bytes).first_non_empty_buffer(0, (16 << 20) + 1),
                    GuestMemory::new(&mut buffer_bytes).first_non_empty_buffer(0, 1),
                ),
                (
                    Err(Failure::Errno(Errno::Nomem)),
                    Err(Failure::Errno(Errno::Nomem))
                )
            ),
            "an iovec array or a buffer past 128 MiB through one preview1 call must answer \
             NOMEM before the host copies any of it"
        );
    }

    // A guest memory of the full 4 GiB lets a field's address pass the end
    // of the address space, which only the arithmetic itself can refuse.
    #[test]
    fn a_field_past_the_address_space_traps_rather_than_wrapping() {
        assert!(
            matches!(GuestMemory::field(u32::MAX - 4, 8), Err(Failure::Trap(_))),
            "a field address past u32::MAX through GuestMemory::field must trap rather than wrap"
        );
    }
}
