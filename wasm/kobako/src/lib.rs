//! kobako — the assembled mruby implementation of the kobako Guest
//! ABI.
//!
//! `MrbGuest` is the harness: one required `init_gems` hook naming
//! the shell-chosen `beni::Gem` set, plus provided `eval` / `run` /
//! `yield_to_block` flows implementing the `kobako_core::Guest`
//! contract over mruby (canonical-boot-state acquisition per
//! invocation, frame reading, codec
//! conversion, block-yield re-entry) and the build-time `bake_boot`
//! hook the wizer pre-initialization entry calls. The crate ships
//! exactly one built-in gem — the wire-tied `KobakoBridge` (Namespace
//! / Handle dispatch + block machinery) — which the boot path
//! installs itself; IO-style capabilities are separate gems (the
//! sibling `kobako-io` crate is the worked example).
//!
//! A shell implements `MrbGuest`, forwards `kobako_core::Guest` to it
//! (the orphan rule keeps that three-line impl in the shell), and
//! emits the wasm exports with `kobako_core::export_guest!`. Any
//! provided flow stays overridable by implementing it in the `Guest`
//! impl instead of forwarding.
//!
//! The mruby-touching internals follow the beni placeholder rule:
//! everything compiles on every target, and operations that need a
//! linked `libmruby.a` are gated on the `mruby_linked` cfg mirrored
//! from `beni-sys` (see `build.rs`) — a placeholder-mode call panics
//! at runtime instead of failing the build.

mod flows;
mod runtime;

pub use runtime::{InstallGroupsError, Kobako};

use beni::{Error, Mrb};

/// The assembled mruby guest as a template: implement `init_gems`
/// and inherit the provided flows.
///
/// The flows install the built-in `KobakoBridge` before running the
/// hook, so `init_gems` names only the shell's additional gems —
/// returning `Ok(())` yields a bridge-only guest. Each provided
/// method matches one `kobako_core::Guest` entry; a shell forwards
/// them in its own `Guest` impl.
pub trait MrbGuest {
    /// Install the shell-chosen gem set onto the freshly booted VM,
    /// via `Mrb::init_gem`. Runs once per boot — at the build-time
    /// bake, or on a non-baked artifact's first entry — after
    /// `KobakoBridge`; an `Err` aborts the boot and surfaces to the
    /// host as a `Kobako::BootError` Panic.
    fn init_gems(mrb: &Mrb) -> Result<(), Error>;

    /// `__kobako_eval` — runs one-shot user source from stdin Frame 2
    /// and writes the Outcome envelope.
    fn eval()
    where
        Self: Sized,
    {
        #[cfg(mruby_linked)]
        {
            flows::eval::<Self>()
        }
        #[cfg(not(mruby_linked))]
        {
            not_linked()
        }
    }

    /// `__kobako_run` — entrypoint dispatch against the invocation
    /// envelope the host wrote into linear memory.
    fn run(env: &[u8])
    where
        Self: Sized,
    {
        #[cfg(mruby_linked)]
        {
            flows::run::<Self>(env)
        }
        #[cfg(not(mruby_linked))]
        {
            let _ = env;
            not_linked()
        }
    }

    /// `__kobako_yield_to_block` — host-initiated re-entry into the
    /// guest block bound to the active dispatch frame.
    fn yield_to_block(req: &[u8]) -> u64
    where
        Self: Sized,
    {
        flows::yield_to_block(req)
    }

    /// Bake the canonical boot state into the
    /// running instance — boot the VM and install the Kobako runtime
    /// plus the shell gem set, leaving preamble installation and
    /// snippet replay to the invocation entries. Called from the
    /// shell's build-time wizer pre-initialization export; panics on
    /// failure so a bake aborts instead of shipping a half-booted
    /// image.
    fn bake_boot()
    where
        Self: Sized,
    {
        #[cfg(mruby_linked)]
        {
            flows::bake_boot::<Self>()
        }
        #[cfg(not(mruby_linked))]
        {
            not_linked()
        }
    }
}

/// Placeholder-mode failure — mirrors `beni`'s runtime semantics for
/// builds without a discovered `libmruby.a`.
#[cfg(not(mruby_linked))]
fn not_linked() -> ! {
    panic!("kobako placeholder mode: mruby is not linked; the provided flows need a discovered libmruby.a")
}
