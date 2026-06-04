//! kobako-wasm — Guest Binary crate root.
//!
//! This crate is the source of `kobako.wasm`, the Guest Binary artifact
//! described in SPEC.md "Core Abstractions". The mruby-free wire tiers
//! (`codec`, `transport`, `outcome`) and the ABI primitives (dispatch
//! import, packed-u64) live in the sibling `kobako-core` contract
//! crate; this crate hosts:
//!
//! * `abi` — Guest ABI surface: the `__kobako_eval` / `__kobako_run` /
//!   `__kobako_alloc` / `__kobako_take_outcome` /
//!   `__kobako_yield_to_block` guest exports (docs/wire-codec.md
//!   § ABI Signatures).
//! * `kobako` — domain runtime: owns the `Kobako` value-token that
//!   installs the `Kobako` module / `Kobako::Transport` / `Kobako::Handle` /
//!   exception classes on an mruby VM and registers the C-bridges in
//!   its `bridges` submodule. No Ruby boot text.
//! * `mruby` — thin façade re-exporting the mruby C-API binding from
//!   the sibling `mruby-sys` crate. Both the raw FFI surface
//!   (`mruby::sys`) and every safe wrapper (`Mrb`, `Ccontext`, the
//!   typed `Value` / `Class` newtypes, `cstr_ptr`) now live in that
//!   crate; this module forwards the existing `use crate::mruby::*`
//!   call-site shape until the consumer code adopts the longer
//!   `crate::mruby::sys::*` paths everywhere, at which point the
//!   façade collapses.
//!
//! The crate uses `std` on every target. `wasm32-wasip1` (the production
//! target — see SPEC.md "Implementation Standards" Architecture) ships a
//! working `std`, including allocator and panic handler. A `no_std` codec
//! is not required by SPEC; switching adds friction (custom allocator,
//! custom panic handler) without buying anything for the Guest Binary,
//! which already pays for `std` through the embedded mruby interpreter.

mod abi;
mod guest;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod kobako;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod mruby;
