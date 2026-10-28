//! The SDK's integration tests, built as one binary so a run links the
//! SDK and its engine once rather than once per file.

// Only the cases behind the `wasmtime` feature load a Guest Binary; the
// others stand in any build.
#[cfg(feature = "wasmtime")]
mod common;

mod byte_surface;
mod concurrency;
mod fillable;
mod flags;
mod install;
mod mount;
mod overrides;
mod runtime_injection;
mod snippet_compile;
mod value_budget;

// Cargo compiles only the modules declared above, so a test file left out
// would never run and nothing would say so.
#[test]
fn every_file_in_this_directory_is_a_declared_module() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/integration");
    for entry in std::fs::read_dir(dir).expect("the test directory must list") {
        let path = entry.expect("an entry must read").path();
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("a UTF-8 name");
        assert!(
            name == "main" || include_str!("main.rs").contains(&format!("mod {name};")),
            "tests/integration/{name} must be declared in main.rs, or it never runs"
        );
    }
}
