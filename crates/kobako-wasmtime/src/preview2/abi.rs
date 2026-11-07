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

scalar!(u8, i8, u16, i16, u32, i32, u64, i64, f32, f64);

fn memory<T>(caller: &mut Caller<'_, T>) -> wasmtime::Result<Memory> {
    match caller.get_export("memory") {
        Some(Extern::Memory(memory)) => Ok(memory),
        _ => Err(wasmtime::Error::msg("the guest exports no memory")),
    }
}

fn read<T>(caller: &mut Caller<'_, T>, at: i32, buf: &mut [u8]) -> wasmtime::Result<()> {
    let memory = memory(caller)?;
    memory
        .read(caller.as_context_mut(), at as u32 as usize, buf)
        .map_err(|_| wasmtime::Error::msg("a value the guest named lies outside its memory"))
}

fn write<T>(caller: &mut Caller<'_, T>, at: i32, bytes: &[u8]) -> wasmtime::Result<()> {
    let memory = memory(caller)?;
    memory
        .write(caller.as_context_mut(), at as u32 as usize, bytes)
        .map_err(|_| wasmtime::Error::msg("an answer's place lies outside the guest's memory"))
}

pub(crate) fn load<S: Scalar>(caller: &mut Caller<'_, impl Sized>, at: i32) -> wasmtime::Result<S> {
    let mut bytes = [0; 8];
    read(caller, at, &mut bytes[..S::SIZE])?;
    Ok(S::from_le(&bytes[..S::SIZE]))
}

pub(crate) fn store<S: Scalar>(
    caller: &mut Caller<'_, impl Sized>,
    at: i32,
    value: S,
) -> wasmtime::Result<()> {
    let mut bytes = [0; 8];
    value.to_le(&mut bytes[..S::SIZE]);
    write(caller, at, &bytes[..S::SIZE])
}

pub(crate) fn load_list<S: Scalar>(
    caller: &mut Caller<'_, impl Sized>,
    at: i32,
    len: i32,
) -> wasmtime::Result<Vec<S>> {
    let size = (len as u32 as usize)
        .checked_mul(S::SIZE)
        .ok_or_else(|| wasmtime::Error::msg("a list the guest named outgrows its memory"))?;
    let mut bytes = vec![0; size];
    read(caller, at, &mut bytes)?;
    Ok((0..size / S::SIZE)
        .map(|i| S::from_le(&bytes[i * S::SIZE..][..S::SIZE]))
        .collect())
}

/// The string at `at`, `len` bytes of UTF-8 as the Canonical ABI encodes one;
/// other bytes trap, as the ABI's lift does.
pub(crate) fn load_string(
    caller: &mut Caller<'_, impl Sized>,
    at: i32,
    len: i32,
) -> wasmtime::Result<String> {
    let mut bytes = vec![0; len as u32 as usize];
    read(caller, at, &mut bytes)?;
    String::from_utf8(bytes).map_err(|_| invalid("string"))
}

/// Room in the guest's memory, from its `cabi_realloc`.
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
    realloc.call(caller.as_context_mut(), (0, 0, align as i32, size))
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
    write(caller, at, &bytes)?;
    Ok((at, len))
}

/// The trap a value the guest encoded wrongly ends the run with.
pub(crate) fn invalid(what: &str) -> wasmtime::Error {
    wasmtime::Error::msg(format!("the guest passed an invalid {what}"))
}

/// The trap a call on a resource the guest never held ends the run with, as
/// the Canonical ABI traps on a handle not in the table.
pub(crate) fn unheld(import: &str) -> wasmtime::Error {
    wasmtime::Error::msg(format!("{import} names a resource the guest does not hold"))
}
