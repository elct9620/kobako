//! kobako-io — the sandbox IO / Kernel capability gem for kobako
//! mruby guests.
//!
//! Installs the write-only `::IO` class, the `STDOUT` / `STDERR`
//! constants, the assignable `$stdout` / `$stderr` globals, and the
//! private Kernel output delegators (`print` / `puts` / `printf` /
//! `p` / `putc` / `warn`) that dispatch through those globals at call
//! time.
//!
//! Pure Rust over `beni`: the entire Ruby-level surface is defined
//! through the typed wrapper — no mrblib, no mrbc / RITE pipeline.
//! The gem is kobako-free; output goes straight to wasi-libc
//! `write(2)`, so any guest shell can compose it. Placeholder-mode
//! builds (host targets without a discovered `libmruby.a`) compile
//! the whole crate; `init` is unreachable there, as beni's
//! `Mrb::open` returns `Err` and no `&Mrb` exists to call it with.

mod io;
mod kernel_ext;

use beni::{Error, Gem, Mrb};

/// The sandbox IO surface as an installable `beni::Gem`.
///
/// Initialization order inside `init` matters: the Kernel delegators
/// dispatch through the `$stdout` / `$stderr` globals, so the IO
/// surface is wired before the delegators register.
pub struct KobakoIo;

impl Gem for KobakoIo {
    fn init(mrb: &Mrb) -> Result<(), Error> {
        io::init(mrb)?;
        kernel_ext::init(mrb)?;
        Ok(())
    }
}
