//! Per-path cache of pre-instantiated wasmtime artifacts.
//!
//! The host imports (`crate::imports`) and their type-check against the
//! compiled Module are identical for every `Kobako::Runtime` on the same
//! Guest Binary. Caching the resolved `InstancePre` per path — together
//! with the ABI version check it guards (`crate::abi`) — takes both off
//! the `Driver::new` hot path.
//!
//! Concurrency: the same as `crate::cache` — the Mutex covers lookup and
//! insert only, and a concurrent miss builds an equivalent `InstancePre`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use wasmtime::InstancePre;

use crate::cache::cached_module;
use crate::invocation::Invocation;
use crate::{abi, imports, trap};
use kobako_runtime::error::SetupError;

static INSTANCE_PRE_CACHE: OnceLock<Mutex<HashMap<PathBuf, InstancePre<Invocation>>>> =
    OnceLock::new();

/// The probe runs only on the first load of a path, so only that load's
/// `setup_timeout` bounds it; a path already verified is not set up again.
pub(crate) fn cached_instance_pre(
    path: &Path,
    setup_timeout: Option<Duration>,
) -> Result<InstancePre<Invocation>, SetupError> {
    let cache = INSTANCE_PRE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Some(pre) = cache
        .lock()
        .expect("instance_pre cache mutex poisoned")
        .get(path)
        .cloned()
    {
        return Ok(pre);
    }

    let module = cached_module(path)?;
    let linker = imports::linker()?;
    let pre = linker
        .instantiate_pre(&module)
        .map_err(trap::instantiate_err)?;
    // Verify before the insert, so a rejected artifact is never
    // remembered as a usable template.
    abi::verify(&pre, setup_timeout)?;
    cache
        .lock()
        .expect("instance_pre cache mutex poisoned")
        .insert(path.to_path_buf(), pre.clone());
    Ok(pre)
}
