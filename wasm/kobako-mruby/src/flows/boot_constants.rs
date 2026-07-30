//! The canonical boot state's top-level constant names, and the
//! subtraction they exist for.
//!
//! An unresolved `#run` target is offered the names it could have been,
//! which are the constants the preloaded snippets contributed. Naming
//! those means knowing what was already there, and that answer belongs
//! to the artifact rather than to the invocation — so it is recorded once
//! during boot, where the wizer bake reaches it, and a `#run` that finds
//! its entrypoint never pays for it.
//!
//! Recording it on the Rust side rather than in an mruby constant keeps
//! it out of the guest's reach: no snippet can read or rewrite the set
//! that decides a correction.
//!
//! The `UnsafeCell` licence and the cross-invocation isolation argument
//! are `super::mrb_slot`'s, unchanged.

use crate::runtime::Kobako;

use core::cell::UnsafeCell;

/// Single-threaded interior-mutability slot for the boot-state constant
/// names. Written once per instance during boot and read only by the
/// unresolved-entrypoint path, so it needs no clear.
struct BootConstantSlot(UnsafeCell<Option<Vec<String>>>);

impl BootConstantSlot {
    const fn new() -> Self {
        Self(UnsafeCell::new(None))
    }

    /// Install `names` as the boot-state snapshot, replacing any previous
    /// value.
    ///
    /// # Safety contract
    ///
    /// No outstanding borrow from `Self::as_ref` may be live. Boot-shaped
    /// use — install once, before any entry body reads — satisfies this.
    fn install(&self, names: Vec<String>) {
        // SAFETY: see type doc — single-threaded wasm execution, and the
        // install happens during boot before any read can borrow.
        unsafe { *self.0.get() = Some(names) };
    }

    /// Borrow the boot-state snapshot if one is installed.
    fn as_ref(&self) -> Option<&[String]> {
        // SAFETY: see type doc.
        unsafe { (*self.0.get()).as_deref() }
    }
}

// SAFETY: wasm32 is single-threaded and the slot is never observed from
// more than one logical owner inside a wasm instance; `static` requires
// `Sync` regardless. Mirrors `super::mrb_slot`'s reasoning.
unsafe impl Sync for BootConstantSlot {}

static BOOT_CONSTANTS: BootConstantSlot = BootConstantSlot::new();

/// Record what `kobako`'s VM holds at top level as the boot-state
/// snapshot. Called from `super::boot::boot_vm` once the runtime is
/// installed — at the bake, or on a non-baked artifact's first entry.
pub(super) fn record(kobako: &Kobako) {
    BOOT_CONSTANTS.install(kobako.top_level_constants());
}

/// The top-level constants the preloaded snippets contributed — the names
/// an unresolved `#run` target could have been. The boot-state snapshot
/// and the namespace each declared bind path roots at are both subtracted,
/// so neither the runtime's own classes nor the Services the preamble
/// materialised read as a snippet's contribution.
pub(super) fn snippet_constants(kobako: &Kobako, bind_paths: &[String]) -> Vec<String> {
    use std::collections::HashSet;

    let boot = BOOT_CONSTANTS
        .as_ref()
        .expect("boot constants recorded by boot_vm alongside the VM");
    let mut installed: HashSet<&str> = boot.iter().map(String::as_str).collect();
    installed.extend(bind_paths.iter().filter_map(|path| path.split("::").next()));

    kobako
        .top_level_constants()
        .into_iter()
        .filter(|name| !installed.contains(name.as_str()))
        .collect()
}
