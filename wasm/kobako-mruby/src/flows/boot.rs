//! Boot helpers shared by the `eval` and `run` downcalls.
//!
//! Both entry points acquire a VM in the canonical boot state —
//! reusing the slot a build-time pre-initialized image baked, or
//! booting lazily — then apply the Preamble's flags, materialise the
//! bound paths' proxy classes, and replay any preloaded snippets before
//! running the entry-specific body. A step that fails
//! before any guest code runs surfaces as a Panic with
//! `origin = sandbox` and `name = "Kobako::BootError"`, except a
//! critical flag the shell does not recognise, which names
//! `Kobako::UnsupportedFlagError` so the host can retry without it.
//!
//! Snippet replay compiles each snippet under a
//! `(snippet:Name)` filename so any uncaught exception's backtrace
//! attributes back to the originating `#preload` call. Replay failures
//! are always sandbox-origin even when the raised class would otherwise
//! map to "service" — preloaded snippets are sandbox code.

use super::panic::{boot_panic, load_panic, panic_from_error, sandbox_panic};
use crate::runtime::Kobako;
use beni::Ccontext;
use beni::Mrb;
use kobako_transport::envelope::{Flag, Origin, Panic, Preamble, Snippet};

/// Every entry starts the same way from its Preamble, so the VM an entry
/// body runs on is set up in one place. Flags come first, since a
/// preloaded snippet may use what a flag turned on.
pub(super) fn enter<G: crate::MrbGuest>(preamble: &Preamble) -> Result<Kobako, Panic> {
    let kobako = boot_with_flags::<G>(preamble)?;
    install_bindings(&kobako, &preamble.bindings.paths)?;
    replay_snippets(&kobako, &preamble.snippets.entries)?;
    Ok(kobako)
}

/// The VM as an invocation has it just before its snippets: booted, with
/// the Preamble's flags applied. A compile starts here, so what it
/// compiles under is what a replay would.
pub(super) fn boot_with_flags<G: crate::MrbGuest>(preamble: &Preamble) -> Result<Kobako, Panic> {
    let kobako = acquire_vm::<G>()?;
    apply_flags::<G>(&kobako, &preamble.flags)?;
    Ok(kobako)
}

/// An unrecognised flag is skipped unless it is critical, so a
/// restriction the host relies on never lapses on a shell that predates it.
fn apply_flags<G: crate::MrbGuest>(kobako: &Kobako, flags: &[Flag]) -> Result<(), Panic> {
    for flag in flags {
        let recognised = G::apply_flag(kobako.mrb(), flag).map_err(|e| {
            boot_panic(format!(
                "failed to apply the flag {:?}: {}",
                flag.name,
                e.message(kobako.mrb())
            ))
        })?;
        if !recognised && flag.critical {
            return Err(sandbox_panic(
                "Kobako::UnsupportedFlagError",
                format!(
                    "this Guest Binary does not support the flag {:?}",
                    flag.name
                ),
            ));
        }
    }
    Ok(())
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

fn install_bindings(kobako: &Kobako, paths: &[String]) -> Result<(), Panic> {
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

fn load_source_snippet(kobako: &Kobako, name: &str, body: &str) -> Result<(), Panic> {
    let filename = snippet_filename(name)?;
    snippet_context(kobako, &filename)?
        .load_nstring(body.as_bytes())
        .map(drop)
        .map_err(|err| replay_panic(load_panic(kobako, &filename, err)))
}

/// The `(snippet:Name)` filename lets a backtrace point back at the
/// originating `#preload` call, whether the snippet replays as source or
/// as the bytecode compiled from it.
pub(super) fn snippet_filename(name: &str) -> Result<std::ffi::CString, Panic> {
    std::ffi::CString::new(format!("(snippet:{})", name))
        .map_err(|_| boot_panic("snippet name contains an invalid character"))
}

pub(super) fn snippet_context<'mrb>(
    kobako: &'mrb Kobako,
    filename: &std::ffi::CStr,
) -> Result<Ccontext<'mrb>, Panic> {
    Ccontext::new(kobako.mrb(), filename)
        .ok_or_else(|| boot_panic("failed to initialize the Sandbox interpreter"))
}

/// A blob that fails its structural check never ran, so it is a
/// `Kobako::BytecodeError`; a program that loaded and then raised keeps the
/// class it raised, `ScriptError` itself included.
fn load_bytecode_snippet(kobako: &Kobako, body: &[u8]) -> Result<(), Panic> {
    let mrb = kobako.mrb();
    // SAFETY: a blob from `compile` is this Guest Binary's own dump; one
    // from `preload(binary:)` is vouched for by the host that supplied it.
    // A blob breaking that promise can reach only this guest's own linear
    // memory, which the host already treats as hostile.
    let program = unsafe { mrb.read_irep(body) }
        .map_err(|err| sandbox_panic("Kobako::BytecodeError", err.message(mrb)))?;
    program
        .call(mrb, &[])
        .map(drop)
        .map_err(|err| replay_panic(panic_from_error(kobako, err)))
}
