//! kobako-regexp — the sandbox Regexp / MatchData capability gem for
//! kobako mruby guests.
//!
//! Backs guest `Regexp` and `MatchData` with the pure-Rust `fancy-regex`
//! engine through the `beni` typed wrapper — no mrblib, no C mrbgem. A
//! guest shell composes it the same way it composes `kobako-io`
//! (docs/behavior.md B-41). Coverage tracks the curated regexp engine's
//! surface, not the full CRuby API; match offsets are byte-based.
//!
//! Placeholder-mode builds (host targets without a discovered
//! `libmruby.a`) compile the whole crate; `init` is unreachable there,
//! as beni's `Mrb::open` returns `Err` and no `&Mrb` exists to call it
//! with.

mod matchdata;
mod regexp;
mod string_ext;
mod translate;

use beni::{Error, Gem, Mrb};

/// The sandbox Regexp surface as an installable `beni::Gem`.
///
/// `init` defines the `Regexp` and `MatchData` classes and the `String`
/// integration methods on the interpreter handle.
pub struct KobakoRegexp;

impl Gem for KobakoRegexp {
    fn init(mrb: &Mrb) -> Result<(), Error> {
        matchdata::init(mrb)?;
        regexp::init(mrb)?;
        string_ext::init(mrb)?;
        Ok(())
    }
}
