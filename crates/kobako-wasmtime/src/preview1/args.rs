//! The checks preview1 makes on an enum or flags argument before a call
//! looks at its descriptor: a value outside what the witx defines is
//! `INVAL`, as wasmtime-wasi's generated bindings answer it.

use super::errno::{Answer, Errno};

/// The bits of `fdflags`.
pub(crate) const FDFLAGS: u64 = 0x1f;
/// The bits of `rights`.
pub(crate) const RIGHTS: u64 = (1 << 30) - 1;
/// The bits of `lookupflags`.
pub(crate) const LOOKUPFLAGS: u64 = 0x1;
/// The bits of `oflags`.
pub(crate) const OFLAGS: u64 = 0xf;
/// The bits of `riflags`.
pub(crate) const RIFLAGS: u64 = 0x3;
/// The bits of `sdflags`.
pub(crate) const SDFLAGS: u64 = 0x3;

/// The highest `advice` variant.
pub(crate) const ADVICE_MAX: u32 = 5;
/// The highest `whence` variant.
pub(crate) const WHENCE_MAX: u32 = 2;

pub(crate) fn flags(value: impl Into<u64>, defined: u64) -> Answer {
    if value.into() & !defined != 0 {
        return Err(Errno::Inval.into());
    }
    Ok(())
}

pub(crate) fn variant(value: u32, max: u32) -> Answer {
    if value > max {
        return Err(Errno::Inval.into());
    }
    Ok(())
}

/// `fstflags`, where setting a timestamp and setting it to now are
/// exclusive.
pub(crate) fn fstflags(value: u32) -> Answer {
    const ATIM: u32 = 1 << 0;
    const ATIM_NOW: u32 = 1 << 1;
    const MTIM: u32 = 1 << 2;
    const MTIM_NOW: u32 = 1 << 3;
    let both = |set, now| value & set != 0 && value & now != 0;
    if value > 0xf || both(ATIM, ATIM_NOW) || both(MTIM, MTIM_NOW) {
        return Err(Errno::Inval.into());
    }
    Ok(())
}
