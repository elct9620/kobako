//! A bytecode snippet read, then run, as two steps.
//!
//! `Mrb::load_bytecode` takes both in one call, as mruby's own
//! `mrb_load_irep_buf` does, and answers a blob it cannot read with the
//! `ScriptError` a running program may also raise. Taking the steps apart
//! over mruby's `mrb_read_irep_buf` tells the two failures apart by when
//! they happened. The run is the one `Mrb::load_bytecode` makes, so a blob
//! that reads runs exactly as it would there.

use beni::sys::{self, FromRawValue};
use beni::{Mrb, ReprValue, Value};

/// Why a bytecode snippet did not load.
pub(super) enum Failure {
    /// The blob failed this structural check, and nothing ran.
    Unreadable(&'static str),
    /// The program loaded, then raised.
    Raised(beni::Error),
}

pub(super) fn load(mrb: &Mrb, bytes: &[u8]) -> Result<(), Failure> {
    // SAFETY: `mrb` is live; `bytes` outlives the synchronous read.
    let irep = unsafe { sys::mrb_read_irep_buf(mrb.as_ptr(), bytes.as_ptr().cast(), bytes.len()) };
    if irep.is_null() {
        return Err(Failure::Unreadable(structural_failure(bytes)));
    }
    // SAFETY: `irep` was just read non-null from a live `mrb`. As mruby's
    // own loader does, it is wrapped in a top-level Proc that takes its
    // ownership, then run at the top self.
    let proc_ = unsafe { sys::mrb_proc_new_func(mrb.as_ptr(), irep) };
    unsafe { (*proc_).c = core::ptr::null_mut() };
    unsafe { sys::mrb_irep_decref(mrb.as_ptr(), irep) };
    let top_self = unsafe { sys::mrb_top_self(mrb.as_ptr()) };
    // A raise inside a running VM long-jumps to this protected frame; one
    // at the top level returns with the exception pending instead.
    sys::protect(mrb, |mrb| unsafe {
        Value::from_raw(sys::mrb_top_run(mrb.as_ptr(), proc_, top_self, 0))
    })
    .map_err(Failure::Raised)?;
    let exc = mrb.pending_exc();
    if exc.is_nil() {
        return Ok(());
    }
    mrb.clear_exc();
    Err(Failure::Raised(beni::Error::Exception(exc)))
}

/// Which structural check the blob failed, read from the RITE binary
/// header (`mruby/dump.h`: ident in bytes 0-3, format version in bytes
/// 4-7). Each constant carries a trailing NUL, so its first four bytes are
/// what the header holds.
fn structural_failure(bytes: &[u8]) -> &'static str {
    if bytes.len() < core::mem::size_of::<sys::rite_binary_header>() {
        return "bytecode shorter than RITE binary header";
    }
    if bytes[..4] != sys::RITE_BINARY_IDENT[..4] {
        return "bytecode header is not RITE format";
    }
    if bytes[4..8] != sys::RITE_BINARY_FORMAT_VER[..4] {
        return "bytecode RITE version mismatch";
    }
    "bytecode body failed structural validation"
}
