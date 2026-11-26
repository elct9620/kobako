//! The bundled Guest Binary every integration test drives, and how a test
//! waits on the thread a guest runs on.

use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use kobako_runtime::sched::{register_blocking, Blocking};
use kobako_runtime::Profile;
use kobako_wasmtime::{Config, Driver};

#[path = "../../../src/guest_binary.rs"]
mod guest_binary;

const WASM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kobako.wasm");

/// A driver over the bundled Guest Binary, or `None` when it has not
/// been built outside CI, where a missing build is a failure instead.
pub fn bundled_driver() -> Option<Driver> {
    bundled_driver_with(|_| {})
}

/// As `bundled_driver`, under the config `adjust` moves.
pub fn bundled_driver_with(adjust: impl FnOnce(&mut Config)) -> Option<Driver> {
    if !guest_binary::built(Path::new(WASM).exists(), "data/kobako.wasm") {
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
    adjust(&mut config);
    Some(Driver::from_file(Path::new(WASM), config).expect("the Guest Binary must load"))
}

thread_local! {
    static COUNTS: RefCell<Option<Arc<AtomicUsize>>> = const { RefCell::new(None) };
}

struct CountsWaits;

impl Blocking for CountsWaits {
    fn wait(&self, wait: &mut dyn FnMut()) {
        COUNTS.with(|counts| {
            if let Some(counts) = counts.borrow().as_ref() {
                counts.fetch_add(1, Ordering::SeqCst);
            }
        });
        wait();
    }
}

/// The longest a test waits for another thread, so what never arrives fails
/// the test instead of hanging the suite.
pub const MAX_WAIT: Duration = Duration::from_secs(10);

/// Wait until `arrived`, sleeping a millisecond between looks so the thread
/// awaited has the CPU, and fail the test with `what` past `MAX_WAIT`.
pub fn wait_until(what: &str, mut arrived: impl FnMut() -> bool) {
    let started = Instant::now();
    while !arrived() {
        assert!(
            started.elapsed() < MAX_WAIT,
            "{what} did not happen within {MAX_WAIT:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Counts each wait the calling thread makes from now on, starting at zero.
pub fn count_waits() -> Arc<AtomicUsize> {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        assert!(
            register_blocking(Box::new(CountsWaits)).is_ok(),
            "another Blocking was registered first; register only this one in the integration tests"
        );
    });
    let counts = Arc::<AtomicUsize>::default();
    COUNTS.with(|held| *held.borrow_mut() = Some(counts.clone()));
    counts
}
