//! The `MrbGuest` provided flows — per-invocation entry bodies over
//! mruby.
//!
//! Each flow implements one `kobako_core::Guest` entry (docs/wire-codec.md
//! § ABI Signatures) for the trait's provided methods: read the stdin
//! invocation frames, acquire the VM in canonical boot state
//! (the baked image, or a lazy boot with the
//! built-in `KobakoBridge` plus the shell-chosen gems), run the
//! entry-specific body, and write the Outcome envelope through
//! `kobako_core::abi`. The `#[no_mangle]` exports themselves are
//! emitted by `kobako_core::export_guest!` in the leaf shell crate.
//!
//! ## Module layout
//!
//! One sibling file per flow alongside the helpers it owns:
//!
//! * `eval` — `__kobako_eval` body.
//! * `run` — `__kobako_run` body.
//! * `yield_block` — `__kobako_yield_to_block` body (host-initiated
//!   re-entry into a guest block).
//! * `boot` — canonical-boot-state acquisition / frame reads / preamble
//!   install / snippet replay / pending-exception extraction helpers
//!   used by both entry points, plus the build-time `bake_boot` body.
//! * `mrb_slot` — module-level static carrying the live VM across the
//!   dispatch re-entry boundary (the block stack lives beside its
//!   bridge writers in `crate::runtime::block_stack`).
//! * `boot_constants` — the boot state's top-level constant names and the
//!   subtraction they exist for, so an unresolved entrypoint can name the
//!   snippet-contributed ones without the successful path paying for it.

#[cfg(any(mruby_linked, test))]
mod boot;
#[cfg(mruby_linked)]
mod boot_constants;
#[cfg(mruby_linked)]
mod eval;
#[cfg(mruby_linked)]
mod mrb_slot;
mod run;
#[cfg(mruby_linked)]
mod yield_block;

#[cfg(mruby_linked)]
pub(crate) use boot::bake_boot;
#[cfg(mruby_linked)]
pub(crate) use eval::eval;
#[cfg(mruby_linked)]
pub(crate) use run::run;
#[cfg(mruby_linked)]
pub(crate) use yield_block::yield_to_block;
