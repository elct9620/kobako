//! RITE / kobako bytecode loaders on `Mrb`.
//!
//! Inherent methods that drop a compiled blob into the live mruby VM
//! and run its top-level Proc.

#[cfg(target_arch = "wasm32")]
use crate::{Mrb, Value};
#[cfg(target_arch = "wasm32")]
use mruby_sys as sys;

#[cfg(target_arch = "wasm32")]
impl Mrb {
    /// `mrb_load_irep_buf(mrb, buf, size)` — load and evaluate a
    /// precompiled RITE bytecode blob. On a malformed blob mruby
    /// sets `mrb->exc`; callers should inspect via
    /// `Mrb::pending_exc` before continuing.
    #[inline]
    pub fn load_irep_buf(&self, bytes: &[u8]) -> Value {
        // SAFETY: `self` is alive; `bytes` is borrowed for the
        // synchronous call.
        Value::from_raw(unsafe {
            sys::mrb_load_irep_buf(
                self.as_ptr(),
                bytes.as_ptr() as *const core::ffi::c_void,
                bytes.len(),
            )
        })
    }

    /// Load + validate + execute a `#preload(binary:)` snippet.
    /// Returns 0 on success and 1 on structural failure (E-37 RITE
    /// version drift / E-38 corrupt or non-RITE body). Top-level
    /// exceptions from a successful load are left in `mrb->exc` for
    /// downstream extraction.
    ///
    /// Wraps the RITE parse step (which mruby keeps separate from
    /// execution via `mrb_read_irep_buf`) with arena bracketing and
    /// a structural-failure classifier; on parse failure a
    /// `RuntimeError` is synthesised under `mrb->exc` so the
    /// caller's existing `take_pending_panic` flow sees a normal
    /// exception. The classifier reads the RITE binary header
    /// directly from `bytes` so the diagnostic distinguishes
    /// "shorter than header" / "wrong ident" / "version mismatch" /
    /// "corrupt body".
    pub fn load_bytecode(&self, bytes: &[u8]) -> core::ffi::c_int {
        // mruby/irep.h documents that `mrb_load_irep*` calls retain
        // one RProc per invocation in the arena; bracketing with
        // save/restore keeps multi-snippet preload cost bounded.
        // mrb->exc is itself a GC root, so any synthesised exception
        // below survives the restore.
        // SAFETY: `self` is alive by the &self borrow.
        let ai = unsafe { sys::mrb_gc_arena_save_func(self.as_ptr()) };

        // SAFETY: bytes pointer is valid for the synchronous call.
        let irep = unsafe {
            sys::mrb_read_irep_buf(
                self.as_ptr(),
                bytes.as_ptr() as *const core::ffi::c_void,
                bytes.len(),
            )
        };

        if irep.is_null() {
            // E-37 (version) or E-38 (corrupt body / non-RITE
            // input). The caller's class-override step folds the
            // synthesised exception into BytecodeError.
            self.set_bytecode_exc(classify_structural_failure(bytes));
            // SAFETY: arena index from the matching save above.
            unsafe { sys::mrb_gc_arena_restore_func(self.as_ptr(), ai) };
            return 1;
        }

        // Mirror mruby's static `load_irep` body: wrap the IREP in
        // a top-level Proc, hand IREP ownership to the Proc via
        // decref, then run. Any top-level raise sets mrb->exc and
        // the caller's existing path picks it up.
        // SAFETY: `irep` was just returned non-null by
        // mrb_read_irep_buf; `mrb` is alive.
        let proc_ = unsafe { sys::mrb_proc_new_func(self.as_ptr(), irep) };
        // SAFETY: `proc_` came from mrb_proc_new and is alive until
        // the matching mrb_top_run consumes it.
        unsafe { (*proc_).c = core::ptr::null_mut() };
        // SAFETY: hands IREP ownership to the Proc.
        unsafe { sys::mrb_irep_decref(self.as_ptr(), irep) };
        // SAFETY: `mrb` is alive.
        let top_self = unsafe { sys::mrb_top_self(self.as_ptr()) };
        // SAFETY: top-level Proc execution; any raise sets mrb->exc.
        unsafe { sys::mrb_top_run(self.as_ptr(), proc_, top_self, 0) };
        // SAFETY: arena index from the matching save above.
        unsafe { sys::mrb_gc_arena_restore_func(self.as_ptr(), ai) };
        0
    }

    /// Set `mrb->exc` to a freshly synthesised `RuntimeError` carrying
    /// `msg`. Used by `Mrb::load_bytecode` to surface structural
    /// failures from `mrb_read_irep_buf` (which signals failure by
    /// returning NULL without setting `mrb->exc`). The caller's
    /// existing pending-exception extraction picks the synthesised
    /// exception up uniformly with mruby-native raises.
    fn set_bytecode_exc(&self, msg: &str) {
        // SAFETY: `self` is alive; `c"RuntimeError"` is a static
        // NUL-terminated literal.
        let runtime_error = unsafe { sys::mrb_class_get(self.as_ptr(), c"RuntimeError".as_ptr()) };
        // SAFETY: `msg` is a Rust string slice borrowed for the
        // synchronous call; mruby copies the bytes into a new
        // exception object.
        let err = Value::from_raw(unsafe {
            sys::mrb_exc_new(
                self.as_ptr(),
                runtime_error,
                msg.as_ptr() as *const core::ffi::c_char,
                msg.len() as sys::mrb_int,
            )
        });
        self.set_pending_exc(err);
    }
}

/// Classify a structural `mrb_read_irep_buf` failure by inspecting
/// the RITE binary header (`mruby/dump.h`: ident in bytes 0–3,
/// format version in bytes 4–7). Returns a stable diagnostic the
/// caller wraps in a `RuntimeError`. The constants come from
/// bindgen-emitted `RITE_BINARY_IDENT` / `RITE_BINARY_FORMAT_VER`
/// (each is a 5-byte slice with a trailing NUL — compare the first
/// 4 bytes against the magic / version bytes the header actually
/// carries).
#[cfg(target_arch = "wasm32")]
fn classify_structural_failure(bytes: &[u8]) -> &'static str {
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
