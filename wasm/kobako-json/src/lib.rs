//! kobako-json — the sandbox JSON capability gem for kobako mruby
//! guests.
//!
//! Backs a guest `JSON` module — `parse`, `generate`, `pretty_generate`
//! — with the pure-Rust `serde_json` engine through the `beni` typed
//! wrapper — no mrblib, no C mrbgem. A guest shell composes it the same
//! way it composes `kobako-io`. Coverage is a curated subset of MRI's
//! `JSON` module, not the full API; the engine is an implementation
//! choice below the capability contract.
//!
//! Placeholder-mode builds (host targets without a discovered
//! `libmruby.a`) compile the whole crate; `init` is unreachable there, as
//! beni's `Mrb::open` returns `Err` and no `&Mrb` exists to call it with.

mod convert;
mod errors;
mod json;

use beni::{Error, Gem, Mrb};

/// The sandbox JSON surface as an installable `beni::Gem`.
///
/// `init` defines the `JSON` error tree, then the `JSON` module with its
/// `parse` / `generate` / `pretty_generate` functions and the
/// `Object#as_json` opt-in hook.
pub struct KobakoJson;

impl Gem for KobakoJson {
    fn init(mrb: &Mrb) -> Result<(), Error> {
        errors::init(mrb)?;
        json::init(mrb)?;
        Ok(())
    }
}
