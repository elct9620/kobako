//! The bundled Guest Binary every integration test drives.

use std::path::Path;
use std::time::Duration;

use kobako_runtime::Profile;
use kobako_wasmtime::{Config, Driver};

const WASM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kobako.wasm");

/// A driver over the bundled Guest Binary, or `None` when it has not
/// been built outside CI, where a missing build is a failure instead.
pub fn bundled_driver() -> Option<Driver> {
    if !Path::new(WASM).exists() {
        assert!(
            std::env::var_os("CI").is_none(),
            "data/kobako.wasm missing under CI — run `bundle exec rake wasm:build`"
        );
        return None;
    }
    Some(
        Driver::new(
            Path::new(WASM),
            Config {
                timeout: Some(Duration::from_secs(10)),
                memory_limit: None,
                stdout_limit: None,
                stderr_limit: None,
                profile: Profile::Hermetic,
            },
        )
        .expect("the Guest Binary must load"),
    )
}
