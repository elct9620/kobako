//! The Guest Binary's declared ABI version check.
//!
//! An artifact reports one ABI version, so the answer belongs to the
//! module and not to the sandbox asking for it. The check therefore runs
//! alongside the `InstancePre` it guards (`crate::instance_pre`) and is
//! amortised with it, rather than once per `Driver::new`.

use wasmtime::{AsContextMut, InstancePre, Store as WtStore};

use crate::cache::shared_engine;
use crate::config::Config;
use crate::invocation::Invocation;
use crate::{frames, trap};
use kobako_runtime::error::SetupError;
use kobako_runtime::profile::Profile;
use kobako_transport::abi::ABI_VERSION;

/// Caps for the throwaway probe instance. Instantiation runs the
/// artifact's start section, so the capture pipes are sized to keep
/// nothing — the probe reads neither — and the strongest isolation rung
/// is taken, since reading an artifact's version grants no ambient
/// authority. The timeout and memory caps stay off because both arm per
/// invocation and there is none here.
const PROBE_CONFIG: Config = Config {
    timeout: None,
    memory_limit: None,
    stdout_limit: Some(0),
    stderr_limit: Some(0),
    profile: Profile::Hermetic,
};

/// Instantiate a throwaway probe instance from `pre` and require the
/// guest's `__kobako_abi_version` export to equal `ABI_VERSION`. An
/// absent export or a non-equal value is a deterministic artifact
/// fault, so every failure is a `SetupError` for the frontend to
/// attribute. The frameless WASI context keeps a third-party guest
/// whose start section touches WASI on the `SetupError` path instead of
/// panicking in `Invocation::wasi_mut`.
pub(crate) fn verify(pre: &InstancePre<Invocation>) -> Result<(), SetupError> {
    let mut store = WtStore::new(shared_engine()?, Invocation::new(None));
    // Epoch interruption is on engine-wide and a fresh Store's deadline
    // has already elapsed, so the probe must name one it cannot reach.
    store.set_epoch_deadline(trap::NO_TIMEOUT_EPOCH_DELTA);
    frames::install_wasi_frames(&mut store, &PROBE_CONFIG, &[])
        .map_err(|t| SetupError::Dead(t.to_string()))?;
    let instance = pre
        .instantiate(store.as_context_mut())
        .map_err(trap::instantiate_err)?;
    let probe = instance
        .get_typed_func::<(), u32>(store.as_context_mut(), "__kobako_abi_version")
        .map_err(|_| {
            SetupError::Dead(format!(
                "the Guest Binary does not export __kobako_abi_version; \
                 rebuild it against ABI version {ABI_VERSION}"
            ))
        })?;
    let reported = probe.call(store.as_context_mut(), ()).map_err(|e| {
        SetupError::Dead(format!(
            "failed to read the Guest Binary's ABI version: {e}"
        ))
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
