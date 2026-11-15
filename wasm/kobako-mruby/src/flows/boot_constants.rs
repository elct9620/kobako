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

use crate::runtime::Kobako;
use kobako_core::instance_cell::InstanceCell;

/// The slot for the boot-state constant names. Written once per instance
/// during boot and read only by the unresolved-entrypoint path, so it
/// needs no clear.
struct BootConstantSlot(InstanceCell<Option<Vec<String>>>);

impl BootConstantSlot {
    const fn new() -> Self {
        Self(InstanceCell::new(None))
    }

    /// Install `names` as the boot-state snapshot, replacing any previous
    /// value.
    ///
    /// # Safety
    ///
    /// No borrow from `Self::as_ref` may be live.
    unsafe fn install(&self, names: Vec<String>) {
        // SAFETY: the caller holds no borrow the write would invalidate.
        unsafe { *self.0.get() = Some(names) };
    }

    /// Borrow the boot-state snapshot if one is installed.
    fn as_ref(&self) -> Option<&[String]> {
        // SAFETY: a write comes only through `install`, whose caller holds
        // no borrow from here.
        unsafe { (*self.0.get()).as_deref() }
    }
}

static BOOT_CONSTANTS: BootConstantSlot = BootConstantSlot::new();

pub(super) fn record(kobako: &Kobako) {
    // SAFETY: boot records before any entry body reads the snapshot.
    unsafe { BOOT_CONSTANTS.install(kobako.top_level_constants()) };
}

/// The top-level constants the preloaded snippets contributed — the names
/// an unresolved `#run` target could have been. The boot-state snapshot
/// and the namespace each declared bind path roots at are both subtracted,
/// so neither the runtime's own classes nor the bound Services read as a
/// snippet's contribution.
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
