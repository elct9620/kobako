//! The Guest Binary's declared ABI version check.
//!
//! An artifact reports one ABI version, so the answer belongs to the
//! module and not to the sandbox asking for it. The check therefore runs
//! alongside the `InstancePre` it guards (`crate::instance_pre`) and is
//! amortised with it, rather than once per `Driver::new`.

use std::time::Duration;

use wasmtime::{AsContextMut, InstancePre, Store as WtStore};

use crate::cache::shared_engine;
use crate::invocation::Invocation;
use crate::trap;
use crate::wasi::Wasi;
use kobako_runtime::error::{SetupError, Trap};
use kobako_transport::abi::ABI_VERSION;

/// An absent or unequal version is a deterministic artifact fault, so
/// every failure is a `SetupError`. `setup_timeout` bounds instantiating
/// the artifact and reading its version, so a start function that never
/// returns fails construction instead of hanging it.
pub(crate) fn verify(
    pre: &InstancePre<Invocation>,
    setup_timeout: Option<Duration>,
) -> Result<(), SetupError> {
    let wasi = Wasi::for_probe().map_err(|t| SetupError::Dead(t.to_string()))?;
    // The memory cap stays off: it arms per invocation, and the probe is
    // none.
    let mut store = WtStore::new(shared_engine()?, Invocation::new(None, wasi));
    trap::arm_deadline(&mut store, setup_timeout);
    let past_deadline = |err: &wasmtime::Error| {
        matches!(trap::cap_trap(err), Some(Trap::Timeout(_))).then(|| {
            SetupError::Dead(format!(
                "the Guest Binary did not finish being set up within {:?}",
                setup_timeout.unwrap_or_default()
            ))
        })
    };
    let instance = pre
        .instantiate(store.as_context_mut())
        .map_err(|e| past_deadline(&e).unwrap_or_else(|| trap::instantiate_err(e)))?;
    let probe = instance
        .get_typed_func::<(), u32>(store.as_context_mut(), "__kobako_abi_version")
        .map_err(|_| {
            SetupError::Dead(format!(
                "the Guest Binary does not export __kobako_abi_version; \
                 rebuild it against ABI version {ABI_VERSION}"
            ))
        })?;
    let reported = probe.call(store.as_context_mut(), ()).map_err(|e| {
        past_deadline(&e).unwrap_or_else(|| {
            SetupError::Dead(format!(
                "failed to read the Guest Binary's ABI version: {e}"
            ))
        })
    })?;
    if reported != ABI_VERSION {
        return Err(SetupError::Dead(format!(
            "the Guest Binary reports ABI version {reported}, but this host \
             implements ABI version {ABI_VERSION}; rebuild the Guest Binary \
             against the host's version"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use kobako_runtime::error::SetupError;
    use kobako_runtime::profile::Profile;

    use crate::{Config, Driver};

    // @behavior RT-112
    #[test]
    fn a_guest_binary_that_never_finishes_instantiating_fails_construction() {
        const FIXTURE: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../test/fixtures/minimal_spinning_start.wat"
        );
        let config = Config {
            timeout: None,
            setup_timeout: Some(Duration::from_millis(300)),
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit: None,
            profile: Profile::Hermetic,
        };
        let built = Driver::new(Path::new(FIXTURE), config);
        assert!(
            matches!(&built, Err(SetupError::Dead(message)) if message.contains("within 300ms")),
            "a Guest Binary whose start never returns through Driver::new must fail construction \
             once the setup deadline passes, saying so"
        );
    }
}
