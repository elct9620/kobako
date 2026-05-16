//! Precompiled mruby bytecode for the kobako guest preamble.
//!
//! Files under `wasm/kobako-wasm/mrblib/` are compiled to RITE-format
//! `.mrb` blobs by `build.rs` (invoking the host-target `mrbc` produced
//! by the same vendored mruby tree as `libmruby.a`). The blobs are
//! embedded into the cdylib via `include_bytes!` and loaded at install
//! time through [`mrb_load_irep_buf`], side-stepping the source-parse
//! cost paid by [`mrb_load_nstring`].
//!
//! ## Why precompile
//!
//! `mrb_load_nstring` runs the full lexer / parser / codegen on every
//! call. For the ~80-line preamble (`io.rb` + `kernel.rb`) that is
//! measurable cold-start cost on every `__kobako_run`. `mrb_load_irep_buf`
//! parses only the RITE header and pulls iseq bytes straight into the
//! VM — the same fast path mruby's own gem mrblib uses.
//!
//! [`mrb_load_irep_buf`]: crate::mruby::sys::mrb_load_irep_buf
//! [`mrb_load_nstring`]: crate::mruby::sys::mrb_load_nstring

#[cfg(target_arch = "wasm32")]
use crate::mruby::sys;

/// Compiled bytecode for `mrblib/io.rb` — defines the instance method
/// surface on the top-level `IO` class (`#print`, `#puts`, `#printf`,
/// `#p`, `#<<`, `#tty?` / `#isatty`, `#sync` / `#sync=`, `#flush`,
/// `#closed?`, `#to_i` alias). Loaded after the C bridges register
/// `IO#write` / `IO#fileno`; see `src/kobako/io.rs`.
#[cfg(target_arch = "wasm32")]
pub(crate) const IO_MRB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/io.mrb"));

/// Compiled bytecode for `mrblib/kernel.rb` — defines `Kernel#print`,
/// `#puts`, `#printf`, `#p`, `#warn` as delegators to the assignable
/// `$stdout` / `$stderr` globals. Loaded after `STDOUT` / `STDERR` /
/// `$stdout` / `$stderr` are wired in `install_raw`.
#[cfg(target_arch = "wasm32")]
pub(crate) const KERNEL_MRB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/kernel.mrb"));

/// Load a precompiled RITE blob into the live mruby state. Returns the
/// last expression value from the loaded code (always `nil` for our
/// preamble files, since both end on a class / module body).
///
/// On a malformed blob (version drift between the linked `libmruby.a`
/// and the host `mrbc` that produced the blob, truncated buffer, etc.)
/// mruby sets `mrb->exc` and returns. Callers that need to surface the
/// fault should inspect the exception via [`kobako_get_exc`] before
/// proceeding. The build pipeline guarantees `mrbc` and `libmruby.a`
/// originate from the same `vendor/mruby/` tree, so under correct
/// builds the load is unconditional.
///
/// # Safety
///
/// `mrb` must be a live mruby state. `bytes` must reference a buffer
/// that lives for the duration of the call; the static `IO_MRB` /
/// `KERNEL_MRB` constants above always satisfy this.
///
/// [`kobako_get_exc`]: crate::mruby::sys::kobako_get_exc
#[cfg(target_arch = "wasm32")]
pub(crate) unsafe fn load(mrb: *mut sys::mrb_state, bytes: &[u8]) {
    unsafe {
        sys::mrb_load_irep_buf(mrb, bytes.as_ptr() as *const core::ffi::c_void, bytes.len());
    }
}

// No host-target stubs needed. Every consumer of `IO_MRB` / `KERNEL_MRB`
// / `load` sits inside a `#[cfg(target_arch = "wasm32")]` block, so the
// items are wasm32-only by reach as well as by definition. Host
// `cargo test` does not compile any reference to them.
