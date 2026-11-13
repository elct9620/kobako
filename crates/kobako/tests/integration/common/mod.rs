//! The real Guest Binary these integration tests drive. A clean checkout
//! has not built it yet, so a missing binary skips locally; CI always
//! builds it first, so a miss there is a broken pipeline and fails.

use std::path::Path;

use kobako::{Options, Sandbox};

#[path = "../../../../kobako-wasmtime/src/guest_binary.rs"]
mod guest_binary;

const WASM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kobako.wasm");

/// A Sandbox over the bundled guest, or `None` when a local checkout has
/// not built it.
pub fn real_sandbox() -> Option<Sandbox> {
    real_sandbox_with(Options::default())
}

/// `real_sandbox` under the caps `options` set.
pub fn real_sandbox_with(options: Options) -> Option<Sandbox> {
    if !guest_binary::built(Path::new(WASM).exists(), "data/kobako.wasm") {
        return None;
    }
    Some(Sandbox::new(WASM, options).expect("construct the Sandbox"))
}
