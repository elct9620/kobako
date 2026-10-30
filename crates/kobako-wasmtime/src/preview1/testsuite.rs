//! Runs wasi-testsuite's prebuilt preview1 tests against kobako's linker, over a
//! Bind of each test's own copy of the directory it is handed. The suite is
//! the second of what decides a preview1 answer, after the witx, so only the
//! calls kobako deliberately refuses may fail, and `testsuite-expected.txt`
//! names them.
//!
//! The suite is fetched by `rake crates:testsuite`, which pins its version, so
//! no gate reaches the network.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use kobako_runtime::filesystem::Bind;
use kobako_runtime::grant::Grant;
use kobako_runtime::mount::{Access, Mount};
use kobako_runtime::profile::Profile;
use kobako_runtime::vfs::Filesystem;
use wasmtime::{Module, Store};
use wasmtime_wasi::I32Exit;

use crate::cache::shared_engine;
use crate::config::Config;
use crate::imports;
use crate::invocation::Invocation;
use crate::trap::arm_deadline;
use crate::wasi::Wasi;

const SUITE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tmp/wasi-testsuite/tests/rust/testsuite/wasm32-wasip1"
);

/// What may fail, one test per line, with why after a `#`.
const EXPECTED: &str = include_str!("testsuite-expected.txt");

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the copy must make its directory");
    for entry in fs::read_dir(from).expect("the suite's directory must list") {
        let entry = entry.expect("the suite's directory must list");
        let target = to.join(entry.file_name());
        match entry.file_type().expect("an entry has a type").is_dir() {
            true => copy_tree(&entry.path(), &target),
            false => {
                fs::copy(entry.path(), target).expect("the suite's file must copy");
            }
        }
    }
}

/// What the test at `wasm` ended with: its exit code, or why it never reached
/// one.
fn run(wasm: &Path, root: &Path) -> Result<i32, String> {
    let engine = shared_engine().map_err(|err| err.to_string())?;
    let bind = Bind::new(root).map_err(|err| err.to_string())?;
    let mount = Mount::new("/", Filesystem::new(Arc::new(bind)), Access::ReadWrite)
        .map_err(|err| err.to_string())?;
    let config = Config {
        timeout: None,
        setup_timeout: None,
        memory_limit: None,
        stdout_limit: None,
        stderr_limit: None,
        open_file_limit: None,
        profile: Profile::Permissive,
    };
    let wasi = Wasi::for_invocation(&config, &[], &[Grant::Mount(mount)])
        .map_err(|err| err.to_string())?;
    let mut store = Store::new(engine, Invocation::new(None, wasi));
    arm_deadline(&mut store, Some(Duration::from_secs(30)));
    let module = Module::from_file(engine, wasm).map_err(|err| err.to_string())?;
    let instance = imports::linker()
        .map_err(|err| err.to_string())?
        .instantiate(&mut store, &module)
        .map_err(|err| err.to_string())?;
    let start = instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .map_err(|err| err.to_string())?;
    match start.call(&mut store, ()) {
        Ok(()) => Ok(0),
        Err(err) => match err.downcast_ref::<I32Exit>() {
            Some(exit) => Ok(exit.0),
            // A test aborts on its first failed assertion, which it wrote to
            // stderr, so that says what the call answered.
            None => Err(String::from_utf8_lossy(&store.data().wasi().stderr_bytes()).to_string()),
        },
    }
}

fn expected_failures() -> BTreeMap<String, String> {
    EXPECTED
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (name, why) = line.split_once('#').unwrap_or((line, ""));
            (name.trim().to_string(), why.trim().to_string())
        })
        .collect()
}

// @behavior RT-192
#[test]
#[ignore = "run through `rake crates:testsuite`, which fetches the suite"]
fn only_what_kobako_refuses_fails_wasi_testsuite() {
    let suite = PathBuf::from(SUITE);
    assert!(
        suite.exists(),
        "wasi-testsuite is not at {SUITE}; run `rake crates:testsuite`"
    );
    let scratch = std::env::temp_dir().join(format!("kobako-testsuite-{}", std::process::id()));
    let mut wasms: Vec<PathBuf> = fs::read_dir(&suite)
        .expect("the suite must list")
        .map(|entry| entry.expect("the suite must list").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "wasm"))
        .collect();
    wasms.sort();

    let mut failed = BTreeMap::new();
    for wasm in &wasms {
        let name = wasm
            .file_stem()
            .expect("a test has a name")
            .to_string_lossy()
            .to_string();
        let root = scratch.join(&name);
        copy_tree(&suite.join("fs-tests.dir"), &root);
        match run(wasm, &root) {
            Ok(0) => {}
            Ok(code) => {
                failed.insert(name, format!("exit {code}"));
            }
            Err(why) => {
                failed.insert(name, why);
            }
        }
    }
    let _ = fs::remove_dir_all(&scratch);

    let expected = expected_failures();
    let unexpected: Vec<_> = failed
        .iter()
        .filter(|(name, _)| !expected.contains_key(*name))
        .collect();
    let passing: Vec<_> = expected
        .keys()
        .filter(|name| !failed.contains_key(*name))
        .collect();
    assert!(
        unexpected.is_empty() && passing.is_empty(),
        "wasi-testsuite through kobako's preview1 imports must fail only what \
         testsuite-expected.txt names; unexpected failures: {unexpected:#?}; now passing: \
         {passing:?}"
    );
}
