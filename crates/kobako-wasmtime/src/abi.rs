//! The Guest Binary's declared ABI version check.
//!
//! An artifact reports one ABI version, so the answer belongs to the
//! module and not to the sandbox asking for it. The check therefore runs
//! alongside the `InstancePre` it guards (`crate::instance_pre`) and is
//! amortised with it, rather than once per `Driver::new`.

use wasmtime::{AsContextMut, InstancePre, Store as WtStore};

use crate::cache::shared_engine;
use crate::invocation::Invocation;
use crate::trap;
use crate::wasi::Wasi;
use kobako_runtime::error::SetupError;
use kobako_transport::abi::ABI_VERSION;

/// An absent or unequal version is a deterministic artifact fault, so
/// every failure is a `SetupError`.
pub(crate) fn verify(pre: &InstancePre<Invocation>) -> Result<(), SetupError> {
    let wasi = Wasi::for_probe().map_err(|t| SetupError::Dead(t.to_string()))?;
    // The memory cap stays off, like the timeout below: both arm per
    // invocation, and the probe is none.
    let mut store = WtStore::new(shared_engine()?, Invocation::new(None, wasi));
    // Epoch interruption is on engine-wide and a fresh Store's deadline
    // has already elapsed, so the probe must name one it cannot reach.
    store.set_epoch_deadline(trap::NO_TIMEOUT_EPOCH_DELTA);
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
