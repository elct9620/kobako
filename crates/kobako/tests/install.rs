//! Integration coverage for the `install` dependency seam: an
//! Extension whose `depends_on` names an uninstalled Extension must fail at
//! the first invocation, before the guest runs, through the real
//! `begin_invocation` path. The unit test on `assert_dependencies` pins the
//! assertion in isolation; only driving `install` -> `eval` on a real
//! Sandbox witnesses that the first invocation reaches it.
//!
//! The dependency assertion raises ahead of the guest, so the guest binary
//! is only needed to construct the Sandbox; the invocation never runs mruby.

// Driven through the bundled engine: these cases load a real Guest Binary,
// so they stand only in a build that carries one.
#![cfg(feature = "wasmtime")]

mod common;

use std::sync::Arc;

use common::real_sandbox;
use kobako::{Error, Extension};

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

// @behavior EX-029 EX-030 EX-041 EX-043
#[test]
fn unmet_dependency_raises_at_first_invocation_naming_the_missing_dependency() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .install(Arc::new(FileExt))
        .expect("install the Extension");

    let err = sandbox
        .eval("1")
        .expect_err("an unmet dependency must fail the first invocation before the guest runs");

    match err {
        Error::Argument(message) => assert!(
            message.contains("File") && message.contains("Errno"),
            "an unmet depends_on must name the Extension and its missing dependency, got: {message}"
        ),
        other => panic!("an unmet dependency must raise Error::Argument, got {other:?}"),
    }
}
