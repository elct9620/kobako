//! The real Guest Binary these integration tests drive. A clean checkout
//! has not built it yet, so a missing binary skips locally; CI always
//! builds it first, so a miss there is a broken pipeline and fails.

use std::path::Path;

use kobako::{Options, Sandbox};

const WASM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kobako.wasm");

/// A Sandbox over the bundled guest, or `None` when a local checkout has
/// not built it.
pub fn real_sandbox() -> Option<Sandbox> {
    if !Path::new(WASM).exists() {
        assert!(
            std::env::var_os("CI").is_none(),
            "data/kobako.wasm missing under CI — run `bundle exec rake wasm:build`"
        );
        return None;
    }
    Some(Sandbox::new(WASM, Options::default()).expect("construct the Sandbox"))
}
