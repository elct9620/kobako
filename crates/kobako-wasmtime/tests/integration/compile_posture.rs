//! A compile reads no host time or entropy, whatever posture the Sandbox
//! requested. Compiling runs no script, so the bundled guest has no way to
//! show what its clock read; a fixture guest whose compile answers the
//! ambient bytes it read shows it instead.

use std::path::Path;

use kobako_runtime::{Profile, Runtime};
use kobako_transport::envelope::{Preamble, Snippet, Snippets};
use kobako_wasmtime::{Config, Driver};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../test/fixtures/minimal_compile_ambient.wat"
);

// @behavior RT-072
#[test]
fn a_compile_reads_no_host_time_or_entropy_under_a_permissive_request() {
    let mut config = Config::default();
    config.timeout = None;
    config.setup_timeout = None;
    config.memory_limit = None;
    config.stdout_limit = None;
    config.stderr_limit = None;
    config.open_file_limit = None;
    config.profile = Profile::Permissive;
    let driver = Driver::new(Path::new(FIXTURE), config).expect("the fixture guest must load");
    let mut preamble = Preamble::default();
    preamble.snippets = Snippets {
        entries: vec![Snippet::Source {
            name: "Only".into(),
            body: "1".into(),
        }],
    };

    assert_eq!(
        driver.compile(&preamble),
        Some(vec![vec![0; 16]]),
        "a permissive request through Driver#compile must read the Unix epoch and the \
         constant stream, not host time and entropy"
    );
}
