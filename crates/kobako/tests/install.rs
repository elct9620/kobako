//! Integration coverage for the `install` dependency seam (E-52): an
//! Extension whose `depends_on` names an uninstalled Extension must fail at
//! the first invocation, before the guest runs, through the real
//! `begin_invocation` path. The unit test on `assert_dependencies` pins the
//! assertion in isolation; only driving `install` -> `eval` on a real
//! Sandbox witnesses that the first invocation reaches it.
//!
//! E-52 raises ahead of the guest, so the guest binary is only needed to
//! construct the Sandbox; the invocation never runs mruby. A missing binary
//! is a hard failure under CI (which always builds it) and a silent skip
//! locally, mirroring the Ruby E2E helper.

use std::path::Path;
use std::sync::Arc;

use kobako::{Error, Extension, Options, Sandbox};

const WASM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kobako.wasm");

/// A guest idiom declaring a dependency the test never installs.
struct FileExt;

impl Extension for FileExt {
    fn name(&self) -> &str {
        "File"
    }

    fn source(&self) -> &str {
        "class File; extend Kobako::Proxy; end"
    }

    fn depends_on(&self) -> &[&str] {
        &["Errno"]
    }
}

#[test]
fn unmet_dependency_raises_at_first_invocation_naming_the_missing_dependency() {
    if !Path::new(WASM).exists() {
        assert!(
            std::env::var_os("CI").is_none(),
            "data/kobako.wasm missing under CI — run `bundle exec rake wasm:build`"
        );
        return;
    }
    let mut sandbox = Sandbox::new(WASM, Options::default()).expect("construct the Sandbox");
    sandbox
        .install(Arc::new(FileExt))
        .expect("install the Extension");

    let err = sandbox.eval("1").expect_err(
        "an unmet dependency must fail the first invocation before the guest runs (E-52)",
    );

    match err {
        Error::Argument(message) => assert!(
            message.contains("File") && message.contains("Errno"),
            "an unmet depends_on must name the Extension and its missing dependency (E-52), got: {message}"
        ),
        other => panic!("an unmet dependency must raise Error::Argument (E-52), got {other:?}"),
    }
}
