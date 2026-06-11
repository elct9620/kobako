//! Process-wide caches for the wasmtime `Engine` and compiled
//! `Module`, plus the on-disk compiled-artifact cache.
//!
//! SPEC.md "Code Organization" pins `ext/` as private and forbids
//! exposing wasm engine types to the Host App or downstream gems. To
//! amortise Engine creation and Module JIT compilation across multiple
//! `Kobako::Sandbox` constructions, the ext keeps a process-scope
//! shared Engine and a per-path Module cache. Both are transparent to
//! Ruby callers, who construct a `Runtime` via
//! `Kobako::Runtime.from_path(...)` and never see Engine or Module.
//!
//! Across processes, the Cranelift compile cost is amortised by a
//! best-effort `.cwasm` disk cache keyed by the SHA-256 of the Guest
//! Binary bytes (docs/behavior.md B-01); every cache failure falls
//! back to in-process compilation.
//!
//! Concurrency: under Ruby's GVL only one thread can execute Rust code
//! at a time, so the Mutex is held briefly during HashMap insert/lookup
//! and serves to satisfy `Sync` bounds rather than to arbitrate real
//! contention.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use magnus::{Error as MagnusError, Ruby};
use sha2::{Digest, Sha256};
use wasmtime::{Config as WtConfig, Engine as WtEngine, Module as WtModule};

use super::{setup_err, MODULE_NOT_BUILT_ERROR};

static SHARED_ENGINE: OnceLock<WtEngine> = OnceLock::new();
static MODULE_CACHE: OnceLock<Mutex<HashMap<PathBuf, WtModule>>> = OnceLock::new();

/// Ticker cadence for the process-singleton epoch ticker. Bounds the
/// granularity of the docs/behavior.md B-01 wall-clock timeout: the
/// `epoch_deadline_callback` fires once per tick (`Continue(1)`), so the
/// trap can lag the deadline by at most one tick under nominal
/// scheduling. 10 ms keeps the lag small enough that it does not skew
/// short test timeouts while leaving the ticker cheap (one wake-up per
/// 10 ms across the whole process).
const EPOCH_TICK: Duration = Duration::from_millis(10);

/// Return the process-wide wasmtime Engine, building it on first call.
///
/// Enables the wasm exceptions proposal so `kobako.wasm` (which uses
/// `try_table` / `exnref` / `tag` for mruby's setjmp-via-new-EH path)
/// can be loaded. The mruby wasi build config uses
/// `-mllvm -wasm-use-legacy-eh=false`, which generates new-style
/// exception handling instructions in the wasm32 object files;
/// wasmtime must have the proposal enabled to parse and JIT those
/// instructions.
///
/// Also enables `epoch_interruption(true)` so every Store can install an
/// `epoch_deadline_callback` for the per-run wall-clock cap
/// (docs/behavior.md B-01, E-19). The first call spawns the process-singleton ticker
/// thread that drives `engine.increment_epoch()` at `EPOCH_TICK`
/// cadence; subsequent calls reuse the same engine and ticker.
pub(crate) fn shared_engine() -> Result<&'static WtEngine, MagnusError> {
    if let Some(engine) = SHARED_ENGINE.get() {
        return Ok(engine);
    }
    let mut config = WtConfig::new();
    config.wasm_exceptions(true);
    config.epoch_interruption(true);
    let engine = WtEngine::new(&config).map_err(|e| {
        let ruby = Ruby::get().expect("Ruby thread");
        setup_err(&ruby, format!("engine init: {}", e))
    })?;
    let engine = SHARED_ENGINE.get_or_init(|| engine);
    spawn_epoch_ticker(engine.clone());
    Ok(engine)
}

/// Spawn the process-singleton epoch ticker. The thread holds a clone of
/// the shared Engine (`wasmtime::Engine` is reference-counted internally)
/// and ticks the epoch counter at `EPOCH_TICK` cadence. Idempotent
/// across reentrant calls to `shared_engine` because `OnceLock`
/// gates the spawn.
fn spawn_epoch_ticker(engine: WtEngine) {
    static TICKER_SPAWNED: OnceLock<()> = OnceLock::new();
    TICKER_SPAWNED.get_or_init(|| {
        thread::Builder::new()
            .name("kobako-epoch-ticker".into())
            .spawn(move || loop {
                thread::sleep(EPOCH_TICK);
                engine.increment_epoch();
            })
            .expect("spawn kobako-epoch-ticker thread");
    });
}

/// Look up `path` in the per-path Module cache, compiling and inserting
/// the artifact on a miss. Raises `Kobako::ModuleNotBuiltError`
/// when the file is missing — the headline error for the common
/// pre-build state on a fresh clone before `rake compile`.
pub(crate) fn cached_module(path: &Path) -> Result<WtModule, MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    let cache = MODULE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Some(module) = cache
        .lock()
        .expect("module cache mutex poisoned")
        .get(path)
        .cloned()
    {
        return Ok(module);
    }

    if !path.exists() {
        return Err(MagnusError::new(
            ruby.get_inner(&MODULE_NOT_BUILT_ERROR),
            format!(
                "Sandbox runtime not found at {}; run `bundle exec rake wasm:build` to build it",
                path.display()
            ),
        ));
    }

    let bytes = fs::read(path).map_err(|e| {
        setup_err(
            &ruby,
            format!(
                "failed to read Sandbox runtime at {}: {}",
                path.display(),
                e
            ),
        )
    })?;
    let engine = shared_engine()?;
    let artifact = artifact_path(&bytes);
    let module = match artifact.as_deref().and_then(|p| load_artifact(engine, p)) {
        Some(module) => module,
        None => {
            let module = WtModule::new(engine, &bytes).map_err(|e| {
                setup_err(&ruby, format!("failed to compile Sandbox runtime: {}", e))
            })?;
            if let Some(p) = artifact.as_deref() {
                store_artifact(&module, p);
            }
            module
        }
    };
    cache
        .lock()
        .expect("module cache mutex poisoned")
        .insert(path.to_path_buf(), module.clone());
    Ok(module)
}

/// Compute the disk-cache location for a Guest Binary's compiled
/// artifact: `$XDG_CACHE_HOME/kobako` (falling back to
/// `~/.cache/kobako`) `/<sha256 of the wasm bytes>.cwasm`. Content
/// addressing makes a rebuilt Guest Binary a new cache entry rather
/// than an invalidation problem; wasmtime itself rejects an artifact
/// produced by an incompatible wasmtime version or Config at
/// deserialize time. Returns `None` when no home directory is
/// available — the caller then just compiles in-process.
fn artifact_path(wasm_bytes: &[u8]) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    let digest = Sha256::digest(wasm_bytes);
    let mut name = String::with_capacity(70);
    for byte in digest {
        let _ = write!(name, "{:02x}", byte);
    }
    name.push_str(".cwasm");
    Some(base.join("kobako").join(name))
}

/// Best-effort load of a previously serialized compiled artifact.
/// Any failure — absent file, truncated bytes, wasmtime version or
/// Config mismatch — returns `None` and the caller recompiles.
fn load_artifact(engine: &WtEngine, artifact: &Path) -> Option<WtModule> {
    if !artifact.exists() {
        return None;
    }
    // SAFETY: `Module::deserialize_file` trusts the artifact bytes.
    // Only files this module wrote into the user-owned cache directory
    // are loaded, addressed by the content hash of the Guest Binary
    // being constructed — an attacker who can plant a file there can
    // already replace the Guest Binary or the gem itself, so the
    // artifact carries exactly the trust of `data/kobako.wasm`.
    unsafe { WtModule::deserialize_file(engine, artifact) }.ok()
}

/// Best-effort write of a freshly compiled artifact. The temp-file +
/// rename pair keeps concurrent processes from observing a partial
/// write; every failure is swallowed because the cache is purely an
/// optimisation.
fn store_artifact(module: &WtModule, artifact: &Path) {
    let Ok(bytes) = module.serialize() else {
        return;
    };
    let Some(dir) = artifact.parent() else { return };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = artifact.with_extension(format!("tmp{}", std::process::id()));
    if fs::write(&tmp, bytes).is_err() {
        return;
    }
    let _ = fs::rename(&tmp, artifact);
}
