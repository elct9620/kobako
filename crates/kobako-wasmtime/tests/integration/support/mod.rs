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
    let mut config = Config::default();
    config.limits.timeout = Some(Duration::from_secs(10));
    config.limits.setup_timeout = Some(Duration::from_secs(10));
    config.limits.memory_limit = None;
    config.limits.stdout_limit = None;
    config.limits.stderr_limit = None;
    config.limits.open_file_limit = None;
    config.profile = Profile::Hermetic;
    Some(Driver::new(Path::new(WASM), config).expect("the Guest Binary must load"))
}
