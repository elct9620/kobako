//! The guest's linear memory as one preview1 call reads what its generated
//! glue left as pointers. An address outside the memory traps, as preview1
//! requires.

use std::cell::Cell;

use wiggle::{GuestPtr, GuestType};

use super::errno::{Answer, Errno};
use super::types;

/// A memory shared between threads hands out no slice to read or write, and
/// no Guest Binary declares one.
fn require_unshared<T>(slice: Option<T>) -> Answer<T> {
    slice.ok_or_else(|| {
        types::Error::trap(wasmtime::Error::msg(
            "a preview1 call needs the guest's memory unshared",
        ))
    })
}

pub(crate) struct GuestMemory<'a, 'm> {
    memory: &'a mut wiggle::GuestMemory<'m>,
    fuel: Cell<u64>,
}

impl<'a, 'm> GuestMemory<'a, 'm> {
    pub(crate) fn new(memory: &'a mut wiggle::GuestMemory<'m>, fuel: usize) -> Self {
        Self {
            memory,
            fuel: Cell::new(fuel as u64),
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

    pub(crate) fn bytes(&self, ptr: GuestPtr<[u8]>) -> Answer<&[u8]> {
        require_unshared(self.memory.as_slice(ptr)?)
    }

    pub(crate) fn bytes_mut(&mut self, ptr: GuestPtr<[u8]>) -> Answer<&mut [u8]> {
        require_unshared(self.memory.as_slice_mut(ptr)?)
    }

    pub(crate) fn read<T: GuestType>(&self, ptr: GuestPtr<T>) -> Answer<T> {
        Ok(self.memory.read(ptr)?)
    }

    pub(crate) fn write<T: GuestType>(&mut self, ptr: GuestPtr<T>, value: T) -> Answer {
        Ok(self.memory.write(ptr, value)?)
    }

    /// Copy `bytes` into guest memory at `ptr`.
    pub(crate) fn write_bytes(&mut self, ptr: GuestPtr<u8>, bytes: &[u8]) -> Answer {
        Ok(self
            .memory
            .copy_from_slice(bytes, ptr.as_array(bytes.len() as u32))?)
    }

    /// A guest path, which preview1 requires to be UTF-8, charged to the
    /// call's fuel before it is read, as wasmtime-wasi charges it.
    pub(crate) fn path(&self, ptr: GuestPtr<str>) -> Answer<String> {
        self.consume(ptr.len(), 1)?;
        let bytes = self.memory.as_cow(ptr.as_bytes())?;
        String::from_utf8(bytes.into_owned()).map_err(|_| Errno::Ilseq.into())
    }

    /// The first non-empty buffer of `iovs`, or an empty one when every
    /// buffer is empty. Like wasmtime-wasi, kobako moves one buffer per call,
    /// and the buffer itself is bounded only once the call has found its
    /// descriptor.
    pub(crate) fn first_non_empty_iovec(&self, iovs: types::IovecArray) -> Answer<GuestPtr<[u8]>> {
        self.first_non_empty(iovs, |iov: types::Iovec| (iov.buf, iov.buf_len))
    }

    /// The first non-empty buffer of `ciovs`, as `first_non_empty_iovec`.
    pub(crate) fn first_non_empty_ciovec(
        &self,
        ciovs: types::CiovecArray,
    ) -> Answer<GuestPtr<[u8]>> {
        self.first_non_empty(ciovs, |ciov: types::Ciovec| (ciov.buf, ciov.buf_len))
    }

    fn first_non_empty<T: GuestType>(
        &self,
        array: GuestPtr<[T]>,
        buffer: impl Fn(T) -> (GuestPtr<u8>, u32),
    ) -> Answer<GuestPtr<[u8]>> {
        self.consume(array.len(), T::guest_size())?;
        for entry in array.iter() {
            let (ptr, len) = buffer(self.memory.read(entry?)?);
            if len != 0 {
                self.consume(len, 1)?;
                return Ok(ptr.as_array(len));
            }
        }
        Ok(GuestPtr::new((0, 0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The most of a guest's memory one call may have the host copy or walk,
    /// which wasmtime hands each call by default.
    const HOSTCALL_FUEL: usize = 128 << 20;

    // Neither the array nor the buffer exists in this small memory, so the
    // budget alone, spent before anything is read, refuses each.
    // @behavior RT-096
    #[test]
    fn a_call_past_its_hostcall_fuel_answers_nomem() {
        // A guest's memory starts aligned, and wiggle refuses an `iovec` at a
        // host address that is not.
        #[repr(align(8))]
        struct AlignedBytes([u8; 64]);

        let mut array_bytes = AlignedBytes([0; 64]);
        let mut buffer_bytes = AlignedBytes([0; 64]);
        buffer_bytes.0[4..8].copy_from_slice(&(128u32 << 20).wrapping_add(1).to_le_bytes());
        let answer = |bytes: &mut [u8], count: u32| {
            let mut memory = wiggle::GuestMemory::Unshared(bytes);
            GuestMemory::new(&mut memory, HOSTCALL_FUEL)
                .first_non_empty_iovec(GuestPtr::new((0, count)))
                .err()
                .and_then(|err| err.downcast().ok())
        };
        assert_eq!(
            (
                answer(&mut array_bytes.0, (16 << 20) + 1),
                answer(&mut buffer_bytes.0, 1)
            ),
            (Some(Errno::Nomem), Some(Errno::Nomem)),
            "an iovec array or a buffer past 128 MiB through one preview1 call must answer \
             NOMEM before the host copies any of it"
        );
    }
}
