//! kobako-io — the sandbox IO / Kernel capability gem for kobako
//! mruby guests.
//!
//! Installs the write-only `::IO` class, the `STDOUT` / `STDERR`
//! constants, the assignable `$stdout` / `$stderr` globals, and the
//! private Kernel output delegators (`print` / `puts` / `printf` /
//! `p` / `putc` / `warn`) that dispatch through those globals at call
//! time (docs/behavior.md B-04 in the kobako repository).
//!
//! Pure Rust over `beni`: the entire Ruby-level surface is defined
//! through the typed wrapper — no mrblib, no mrbc / RITE pipeline.
//! The gem is kobako-free; output goes straight to wasi-libc
//! `write(2)`, so any guest shell can compose it.
//!
//! The mruby-touching internals are gated on the `mruby_linked` cfg
//! mirrored from `beni-sys` (see `build.rs`); placeholder-mode builds
//! (host targets without a discovered `libmruby.a`) compile only this
//! public surface.

#[cfg(mruby_linked)]
mod io;
#[cfg(mruby_linked)]
mod kernel;

use beni::{Error, Gem, Mrb};

/// The sandbox IO surface as an installable `beni::Gem`.
///
/// Install order inside `init` matters: the Kernel delegators look up
/// `$stdout` / `$stderr` at call time, so the `IO` class and the
/// globals are wired before the delegators register.
pub struct KobakoIo;

impl Gem for KobakoIo {
    #[cfg(mruby_linked)]
    fn init(mrb: &Mrb) -> Result<(), Error> {
        io::install(mrb)?;
        io::install_globals(mrb)?;
        kernel::install(mrb);
        Ok(())
    }

    #[cfg(not(mruby_linked))]
    fn init(_mrb: &Mrb) -> Result<(), Error> {
        panic!("kobako-io placeholder mode: mruby is not linked; install needs a discovered libmruby.a")
    }
}
