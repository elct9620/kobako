//! kobako-wasm — Guest Binary crate root.
//!
//! This crate is the source of `kobako.wasm`, the Guest Binary
//! artifact described in SPEC.md "Core Abstractions". It is the leaf
//! shell over the published guest stack: `kobako-mruby` supplies the
//! `MrbGuest` harness (provided flows + the built-in `KobakoBridge`
//! gem), `kobako-io` the IO / Kernel capability gem, and
//! `kobako-core` the ABI contract whose `export_guest!` macro emits
//! the wasm exports here — the exact composition path any
//! third-party guest takes.

mod guest;

/// Build-time wizer pre-initialization entry: bakes the canonical boot
/// state into the artifact's memory image.
/// Stage C runs wizer over the linked module and this function is
/// consumed there — it is never called at Sandbox runtime.
#[export_name = "wizer.initialize"]
pub extern "C" fn wizer_initialize() {
    <guest::KobakoGuest as kobako_mruby::MrbGuest>::bake_boot();
}
