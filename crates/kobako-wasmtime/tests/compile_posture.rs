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
    let driver = Driver::new(
        Path::new(FIXTURE),
        Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit: None,
            profile: Profile::Permissive,
        },
    )
    .expect("the fixture guest must load");
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
