//! kobako-io — the sandbox IO / Kernel capability gem for kobako
//! mruby guests.
//!
//! Installs the `::IO` class, the `STDOUT` / `STDERR` constants, the
//! assignable `$stdout` / `$stderr` globals, `::File < IO` over the
//! directories the host preopens, the errors a file operation raises, and
//! the private Kernel output delegators (`print` / `puts` / `printf` /
//! `p` / `putc` / `warn`) that dispatch through those globals at call
//! time.
//!
//! Pure Rust over `beni`: the entire Ruby-level surface is defined
//! through the typed wrapper — no mrblib, no mrbc / RITE pipeline.
//! The gem is kobako-free; it reaches the host only through wasi-libc,
//! so any guest shell can compose it.

mod descriptors;
mod errno;
mod file;
mod io;
mod kernel_ext;
mod stream;

use beni::{Error, Gem, Mrb};

/// The sandbox IO surface as an installable `beni::Gem`.
///
/// Initialization order inside `init` matters: the Kernel delegators
/// dispatch through the `$stdout` / `$stderr` globals, so the IO
/// surface is wired before the delegators register.
pub struct KobakoIo;

impl Gem for KobakoIo {
    fn init(mrb: &Mrb) -> Result<(), Error> {
        errno::init(mrb)?;
        let io = io::init(mrb)?;
        stream::init(mrb, io)?;
        file::init(mrb, io)?;
        kernel_ext::init(mrb)?;
        Ok(())
    }
}
