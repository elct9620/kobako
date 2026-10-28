//! The driver's integration tests, built as one binary so a run links
//! wasmtime once rather than once per file.

mod support;

mod compile_posture;
mod guest_file;
mod preamble_flags;
mod snippet_compile;
mod unanswered_call;
mod unsupported_call;

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
