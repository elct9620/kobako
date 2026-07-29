//! kv-guest — the Guest Binary this example's host runs.
//!
//! A leaf shell over the published guest stack: `kobako-mruby` supplies
//! the interpreter harness, `kobako-core` the ABI contract whose
//! `export_guest!` emits the wasm exports, and this crate supplies the
//! two things the harness leaves open — the payload schema and the gem
//! set.

mod codec;
mod guest;

/// Build-time entry the bake tool calls to pre-initialise the booted
/// interpreter into the artifact's memory image. Never called at
/// runtime; an unbaked artifact boots on its first invocation instead.
#[export_name = "wizer.initialize"]
pub extern "C" fn wizer_initialize() {
    <guest::KvGuest as kobako_mruby::MrbGuest>::bake_boot();
}
