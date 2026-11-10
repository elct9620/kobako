//! What the generated bindings read and write in the guest's memory, as the
//! Canonical ABI lays a value out: little-endian scalars at their offset, and
//! lists the guest's `cabi_realloc` allocates.

use wasmtime::{AsContextMut, Caller, Extern, Memory};

/// A scalar the Canonical ABI stores little-endian at its natural size.
pub(crate) trait Scalar: Copy {
    const SIZE: usize;
    fn from_le(bytes: &[u8]) -> Self;
    fn to_le(self, bytes: &mut [u8]);
}

macro_rules! scalar {
    ($($ty:ty),*) => {$(
        impl Scalar for $ty {
            const SIZE: usize = std::mem::size_of::<$ty>();
            fn from_le(bytes: &[u8]) -> Self {
                <$ty>::from_le_bytes(bytes.try_into().expect("a scalar's width"))
            }
            fn to_le(self, bytes: &mut [u8]) {
                bytes.copy_from_slice(&self.to_le_bytes());
            }
        }
    )*};
}

scalar!(u8, i8, u16, i16, u32, i32, u64, i64);

fn memory<T>(caller: &mut Caller<'_, T>) -> wasmtime::Result<Memory> {
    match caller.get_export("memory") {
        Some(Extern::Memory(memory)) => Ok(memory),
        _ => Err(wasmtime::Error::msg("the guest exports no memory")),
    }
}

/// The guest address `offset` bytes past `at`, a 32-bit pointer as the
/// Canonical ABI reads one; widened so the sum never wraps into memory the
/// guest did not name.
fn address(at: i32, offset: usize) -> usize {
    at as u32 as usize + offset
}

fn read<T>(caller: &mut Caller<'_, T>, at: usize, buf: &mut [u8]) -> wasmtime::Result<()> {
    let memory = memory(caller)?;
    memory
        .read(caller.as_context_mut(), at, buf)
        .map_err(|_| wasmtime::Error::msg("a value the guest named lies outside its memory"))
}

fn write<T>(caller: &mut Caller<'_, T>, at: usize, bytes: &[u8]) -> wasmtime::Result<()> {
    let memory = memory(caller)?;
    memory
        .write(caller.as_context_mut(), at, bytes)
        .map_err(|_| wasmtime::Error::msg("an answer's place lies outside the guest's memory"))
}

/// The return area the guest names at `at`, which must sit on its answer's
/// `align`, as the Canonical ABI traps on a misaligned one before an answer
/// is written.
pub(crate) fn ensure_aligned(at: i32, align: usize) -> wasmtime::Result<()> {
    match address(at, 0).is_multiple_of(align) {
        true => Ok(()),
        false => Err(wasmtime::Error::msg(
            "the return area the guest named is not aligned",
        )),
    }
}

pub(crate) fn load<S: Scalar>(
    caller: &mut Caller<'_, impl Sized>,
    at: i32,
    offset: usize,
) -> wasmtime::Result<S> {
    let mut bytes = [0; 8];
    read(caller, address(at, offset), &mut bytes[..S::SIZE])?;
    Ok(S::from_le(&bytes[..S::SIZE]))
}

pub(crate) fn store<S: Scalar>(
    caller: &mut Caller<'_, impl Sized>,
    at: i32,
    offset: usize,
    value: S,
) -> wasmtime::Result<()> {
    let mut bytes = [0; 8];
    value.to_le(&mut bytes[..S::SIZE]);
    write(caller, address(at, offset), &bytes[..S::SIZE])
}

/// The length of the list of `len` elements of `size` bytes the guest names
/// at `at`, once it lies wholly inside the guest's memory and on its `align`,
/// as wasmtime's `WasmList::new` checks before anything is read; a list
/// outside traps, so no length the guest names sizes a host allocation first.
pub(crate) fn list_len(
    caller: &mut Caller<'_, impl Sized>,
    at: i32,
    len: i32,
    size: usize,
    align: usize,
) -> wasmtime::Result<usize> {
    let len = len as u32 as usize;
    let end = len
        .checked_mul(size)
        .and_then(|bytes| address(at, 0).checked_add(bytes));
    let memory = memory(caller)?;
    match end {
        Some(end) if end <= memory.data_size(caller.as_context_mut()) => {}
        _ => {
            return Err(wasmtime::Error::msg(
                "a list the guest named lies outside its memory",
            ))
        }
    }
    if !address(at, 0).is_multiple_of(align) {
        return Err(wasmtime::Error::msg(
            "a list the guest named is not aligned",
        ));
    }
    Ok(len)
}

/// What one call may have the host copy out of the guest's memory, the
/// hostcall fuel wasmtime hands each host call, as preview1's calls spend it.
/// Each list a call lifts spends its bytes first, so lists naming one region
/// of memory over and over cannot multiply what the host holds.
pub(crate) struct Fuel(usize);

impl Fuel {
    pub(crate) fn new(caller: &mut Caller<'_, impl Sized>) -> Self {
        Self(caller.as_context_mut().hostcall_fuel())
    }

    /// Spend `len` elements of `size` bytes, or trap once the call's fuel is
    /// gone, as wasmtime's `consume_fuel_array`.
    pub(crate) fn spend(&mut self, len: usize, size: usize) -> wasmtime::Result<()> {
        self.0 = len
            .checked_mul(size)
            .and_then(|bytes| self.0.checked_sub(bytes))
            .ok_or_else(|| {
                wasmtime::Error::msg("the guest had a call copy more than its hostcall fuel allows")
            })?;
        Ok(())
    }
}

pub(crate) fn load_list<S: Scalar>(
    caller: &mut Caller<'_, impl Sized>,
    fuel: &mut Fuel,
    at: i32,
    len: i32,
) -> wasmtime::Result<Vec<S>> {
    let len = list_len(caller, at, len, S::SIZE, S::SIZE)?;
    fuel.spend(len, S::SIZE)?;
    let mut bytes = vec![0; len * S::SIZE];
    read(caller, address(at, 0), &mut bytes)?;
    Ok((0..len)
        .map(|i| S::from_le(&bytes[i * S::SIZE..][..S::SIZE]))
        .collect())
}

/// The string at `at`, `len` bytes of UTF-8 as the Canonical ABI encodes one;
/// other bytes trap, as the ABI's lift does.
pub(crate) fn load_string(
    caller: &mut Caller<'_, impl Sized>,
    fuel: &mut Fuel,
    at: i32,
    len: i32,
) -> wasmtime::Result<String> {
    let len = list_len(caller, at, len, 1, 1)?;
    fuel.spend(len, 1)?;
    let mut bytes = vec![0; len];
    read(caller, address(at, 0), &mut bytes)?;
    String::from_utf8(bytes).map_err(|_| invalid("string"))
}

/// Room for `size` bytes on `align` in the guest's memory, from its
/// `cabi_realloc`; room outside its memory or off its alignment traps, as
/// wasmtime checks `realloc`'s return.
pub(crate) fn realloc(
    caller: &mut Caller<'_, impl Sized>,
    size: i32,
    align: usize,
) -> wasmtime::Result<i32> {
    let realloc = caller
        .get_export("cabi_realloc")
        .and_then(Extern::into_func)
        .ok_or_else(|| wasmtime::Error::msg("the guest exports no cabi_realloc"))?
        .typed::<(i32, i32, i32, i32), i32>(caller.as_context_mut())?;
    let at = realloc.call(caller.as_context_mut(), (0, 0, align as i32, size))?;
    list_len(caller, at, size, 1, align)?;
    Ok(at)
}

pub(crate) fn store_list<S: Scalar>(
    caller: &mut Caller<'_, impl Sized>,
    list: &[S],
    align: usize,
) -> wasmtime::Result<(i32, i32)> {
    let mut bytes = vec![0; list.len() * S::SIZE];
    for (i, value) in list.iter().enumerate() {
        value.to_le(&mut bytes[i * S::SIZE..][..S::SIZE]);
    }
    let len = i32::try_from(list.len())
        .map_err(|_| wasmtime::Error::msg("a list too long for the guest"))?;
    let at = realloc(caller, bytes.len() as i32, align)?;
    write(caller, address(at, 0), &bytes)?;
    Ok((at, len))
}

/// The trap a value the guest encoded wrongly ends the run with.
pub(crate) fn invalid(what: &str) -> wasmtime::Error {
    wasmtime::Error::msg(format!("the guest passed an invalid {what}"))
}
