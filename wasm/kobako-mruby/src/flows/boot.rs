//! Boot helpers shared by `__kobako_eval` and `__kobako_run`.
//!
//! Both entry points acquire a VM in the canonical boot state —
//! reusing the slot a build-time pre-initialized image baked, or
//! booting lazily — then materialise
//! the Frame 1 preamble's proxy classes and replay any preloaded Frame 3
//! snippets before running the entry-specific body. When any of those
//! steps fails, the failure surfaces as a Panic with
//! `origin = sandbox` and `name = "Kobako::BootError"`.
//!
//! Snippet replay compiles each snippet under a
//! `(snippet:Name)` filename so any uncaught exception's backtrace
//! attributes back to the originating `#preload` call. Replay failures
//! are always sandbox-origin even when the raised class would otherwise
//! map to "service" — preloaded snippets are sandbox code.

use super::panic::{boot_panic, load_panic, panic_from_error};
use crate::runtime::Kobako;
use beni::Ccontext;
use beni::Mrb;
use kobako_transport::envelope::{Bindings, ErrorRecord, Origin, Panic, Snippet, Snippets};

pub(super) fn read_preamble() -> Result<Vec<String>, Panic> {
    let bytes = kobako_core::frames::read_frame()
        .ok_or_else(|| boot_panic("failed to read the Sandbox setup data"))?;
    Bindings::decode(&bytes)
        .map(|bindings| bindings.paths)
        .map_err(|_| boot_panic("failed to decode the Sandbox setup data"))
}

pub(super) fn read_snippets() -> Result<Vec<Snippet>, Panic> {
    let bytes = kobako_core::frames::read_frame()
        .ok_or_else(|| boot_panic("failed to read the preloaded snippets"))?;
    Snippets::decode(&bytes)
        .map(|snippets| snippets.entries)
        .map_err(|_| boot_panic("failed to decode the preloaded snippets"))
}

/// Every entry starts the same way once its frames are read, so the VM an
/// entry body runs on is set up in one place.
pub(super) fn enter<G: crate::MrbGuest>(
    preamble: &[String],
    snippets: &[Snippet],
) -> Result<Kobako, Panic> {
    let kobako = acquire_vm::<G>()?;
    install_preamble(&kobako, preamble)?;
    replay_snippets(&kobako, snippets)?;
    Ok(kobako)
}

/// On `Err` the slot is cleared, so no caller observes a half-set state.
fn boot_vm<G: crate::MrbGuest>() -> Result<(), Panic> {
    let mrb = Mrb::open().map_err(|_| boot_panic("failed to start the Sandbox interpreter"))?;
    super::mrb_slot::MRB.install(mrb);
    let mrb = super::mrb_slot::MRB
        .as_ref()
        .expect("MRB just installed above");
    let kobako = match Kobako::init::<G>(mrb) {
        Ok(kobako) => kobako,
        Err(e) => {
            let panic = boot_panic(format!(
                "Sandbox boot registration failed: {}",
                e.message(mrb)
            ));
            super::mrb_slot::MRB.clear();
            return Err(panic);
        }
    };
    // Recorded on the success path only: an unresolved entrypoint's
    // correction is measured against this, and the bake reaches here.
    super::boot_constants::record(&kobako);
    Ok(())
}

/// Reuses the VM the pre-initialized image baked, or boots lazily when the
/// artifact carries none.
fn acquire_vm<G: crate::MrbGuest>() -> Result<Kobako, Panic> {
    if super::mrb_slot::MRB.as_ref().is_none() {
        boot_vm::<G>()?;
    }
    let mrb = super::mrb_slot::MRB
        .as_ref()
        .expect("slot populated by the baked image or boot_vm above");
    // SAFETY: the slot only ever holds a VM that passed `Kobako::init`
    // — baked at build time (`bake_boot`) or booted by `boot_vm` above.
    Ok(unsafe { Kobako::resolve_raw(mrb) })
}

/// Panics on failure, so a bake aborts loudly instead of shipping a
/// half-booted image.
pub(crate) fn bake_boot<G: crate::MrbGuest>() {
    if let Err(panic) = boot_vm::<G>() {
        panic!("canonical boot state bake failed: {}", panic.error.message);
    }
}

fn install_preamble(kobako: &Kobako, paths: &[String]) -> Result<(), Panic> {
    kobako
        .install_bindings(paths)
        .map_err(|err| boot_panic(err.to_string()))
}

/// A failing snippet's Panic is forced to sandbox origin even when its
/// class would choose the Service: preloaded snippets are sandbox code.
fn replay_snippets(kobako: &Kobako, snippets: &[Snippet]) -> Result<(), Panic> {
    for entry in snippets {
        match entry {
            Snippet::Source { name, body } => load_source_snippet(kobako, name, body)?,
            Snippet::Bytecode { body } => load_bytecode_snippet(kobako, body)?,
        }
    }
    Ok(())
}

fn replay_panic(panic: Panic) -> Panic {
    Panic {
        origin: Origin::Sandbox,
        ..panic
    }
}

/// The `(snippet:Name)` filename lets a backtrace point back at the
/// originating `#preload` call.
fn load_source_snippet(kobako: &Kobako, name: &str, body: &str) -> Result<(), Panic> {
    let filename = std::ffi::CString::new(format!("(snippet:{})", name))
        .map_err(|_| boot_panic("snippet name contains an invalid character"))?;
    let Some(cxt) = Ccontext::new(kobako.mrb(), &filename) else {
        return Err(boot_panic("failed to initialize the Sandbox interpreter"));
    };
    cxt.load_nstring(body.as_bytes())
        .map(drop)
        .map_err(|err| replay_panic(load_panic(kobako, &filename, err)))
}

/// The class a bytecode load answers when the blob fails its
/// structural check — `ScriptError` itself, so a subclass the
/// program raised keeps its own name.
const STRUCTURAL_FAILURE: &str = "ScriptError";

/// A blob that fails its structural check is promoted to
/// `Kobako::BytecodeError`; a program that loaded and then raised keeps the
/// class it raised.
fn load_bytecode_snippet(kobako: &Kobako, body: &[u8]) -> Result<(), Panic> {
    let Err(err) = kobako.mrb().load_bytecode(body) else {
        return Ok(());
    };
    let panic = replay_panic(panic_from_error(kobako, err));
    if panic.error.name != STRUCTURAL_FAILURE {
        return Err(panic);
    }
    Err(Panic {
        error: ErrorRecord {
            name: "Kobako::BytecodeError".into(),
            ..panic.error
        },
        ..panic
    })
}
