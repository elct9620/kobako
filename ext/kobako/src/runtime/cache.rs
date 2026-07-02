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
//! Binary bytes; every cache failure falls
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
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};
use wasmtime::{Config as WtConfig, Engine as WtEngine, Module as WtModule};

use kobako_runtime::error::SetupError;

static SHARED_ENGINE: OnceLock<WtEngine> = OnceLock::new();
static MODULE_CACHE: OnceLock<Mutex<HashMap<PathBuf, WtModule>>> = OnceLock::new();

/// Ticker cadence for the process-singleton epoch ticker. Bounds the
/// granularity of the wall-clock timeout: the
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
/// cap. The first call spawns the process-singleton ticker
/// thread that drives `engine.increment_epoch()` at `EPOCH_TICK`
/// cadence; subsequent calls reuse the same engine and ticker.
pub(super) fn shared_engine() -> Result<&'static WtEngine, SetupError> {
    if let Some(engine) = SHARED_ENGINE.get() {
        return Ok(engine);
    }
    let mut config = WtConfig::new();
    config.wasm_exceptions(true);
    config.epoch_interruption(true);
    let engine =
        WtEngine::new(&config).map_err(|e| SetupError::Dead(format!("engine init: {e}")))?;
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
/// the artifact on a miss. Returns `SetupError::ModuleNotBuilt`
/// (boundary → `Kobako::ModuleNotBuiltError`) when the file is missing —
/// the headline error for the common pre-build state on a fresh clone
/// before `rake compile`.
pub(super) fn cached_module(path: &Path) -> Result<WtModule, SetupError> {
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
        return Err(SetupError::ModuleNotBuilt(format!(
            "Sandbox runtime not found at {}; run `bundle exec rake wasm:build` to build it",
            path.display()
        )));
    }

    let bytes = fs::read(path).map_err(|e| {
        SetupError::Dead(format!(
            "failed to read Sandbox runtime at {}: {e}",
            path.display()
        ))
    })?;
    let engine = shared_engine()?;
    let artifact = artifact_path(&bytes);
    let module = match artifact.as_deref().and_then(|p| load_artifact(engine, p)) {
        Some(module) => module,
        None => {
            let module = WtModule::new(engine, &bytes)
                .map_err(|e| SetupError::Dead(format!("failed to compile Sandbox runtime: {e}")))?;
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

/// Retention window for unused cache entries. A hit refreshes the
/// artifact's mtime, so only entries no process has loaded for the
/// whole window are removed by `prune_stale`.
const ARTIFACT_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Compute the disk-cache location for a Guest Binary's compiled
/// artifact: `$XDG_CACHE_HOME/kobako` (falling back to
/// `~/.cache/kobako`) `/<sha256 of the wasm bytes>-<gem version>.cwasm`.
/// Content addressing makes a rebuilt Guest Binary a new cache entry
/// rather than an invalidation problem; the gem-version segment keeps
/// two installed kobako versions (each pinning its own wasmtime) from
/// sharing a key and recompile-thrashing each other's entry. wasmtime
/// itself rejects an artifact produced by an incompatible wasmtime
/// version or Config at deserialize time. Returns `None` when no home
/// directory is available — the caller then just compiles in-process.
fn artifact_path(wasm_bytes: &[u8]) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    let digest = Sha256::digest(wasm_bytes);
    let mut name = String::with_capacity(80);
    for byte in digest {
        let _ = write!(name, "{:02x}", byte);
    }
    let _ = write!(name, "-{}.cwasm", env!("CARGO_PKG_VERSION"));
    Some(base.join("kobako").join(name))
}

/// Best-effort load of a previously serialized compiled artifact.
/// Any failure — absent file, truncated bytes, wasmtime version or
/// Config mismatch — returns `None` and the caller recompiles. A hit
/// refreshes the file's mtime so `prune_stale`'s retention window
/// measures time since last use, not since creation.
fn load_artifact(engine: &WtEngine, artifact: &Path) -> Option<WtModule> {
    if !artifact.exists() || !artifact.parent().is_some_and(dir_is_private) {
        return None;
    }
    // SAFETY: `Module::deserialize_file` trusts the artifact bytes.
    // `dir_is_private` just verified the cache directory is owned by
    // the current user and writable by no one else, so only files this
    // module wrote are loaded, addressed by the content hash of the
    // Guest Binary being constructed — the artifact carries exactly
    // the trust of `data/kobako.wasm`.
    let module = unsafe { WtModule::deserialize_file(engine, artifact) }.ok()?;
    let _ = fs::File::options()
        .append(true)
        .open(artifact)
        .and_then(|f| f.set_modified(SystemTime::now()));
    Some(module)
}

/// Best-effort write of a freshly compiled artifact. The temp-file +
/// rename pair keeps concurrent processes from observing a partial
/// write; every failure is swallowed because the cache is purely an
/// optimisation. A successful write also triggers `prune_stale` so the
/// cache directory cannot grow without bound across Guest Binary
/// rebuilds.
fn store_artifact(module: &WtModule, artifact: &Path) {
    let Ok(bytes) = module.serialize() else {
        return;
    };
    let Some(dir) = artifact.parent() else { return };
    if create_cache_dir(dir).is_err() || !dir_is_private(dir) {
        return;
    }
    let tmp = artifact.with_extension(format!("tmp{}", std::process::id()));
    if fs::write(&tmp, bytes).is_err() {
        return;
    }
    if fs::rename(&tmp, artifact).is_ok() {
        prune_stale(dir, artifact);
    }
}

/// Create the cache directory owner-only (`0700`) on Unix so no other
/// local user can plant an artifact the unsafe deserialize would
/// trust; elsewhere fall back to default permissions.
#[cfg(unix)]
fn create_cache_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_cache_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)
}

/// Returns whether the cache directory upholds the trust the unsafe
/// deserialize relies on: owned by the current effective user and
/// writable by no one else. A pre-existing directory another user owns
/// or can write to — e.g. under a shared `XDG_CACHE_HOME` — fails here
/// and both disk-cache tiers are skipped.
#[cfg(unix)]
fn dir_is_private(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(meta) = fs::metadata(dir) else {
        return false;
    };
    // SAFETY: `geteuid` reads process state and has no preconditions.
    meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o022 == 0
}

#[cfg(not(unix))]
fn dir_is_private(_dir: &Path) -> bool {
    true
}

/// Remove every cache entry (`.cwasm` artifacts and crash-leftover
/// `.tmp*` files) whose mtime sits past `ARTIFACT_TTL`, except the
/// just-written `keep`. Live temp files are seconds old and never
/// qualify; foreign file names are left untouched.
fn prune_stale(dir: &Path, keep: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep || !cache_entry_name(&path) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|mtime| mtime.elapsed().ok())
            .is_some_and(|age| age > ARTIFACT_TTL);
        if stale {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Returns whether `path` carries a file name this cache wrote — a
/// `.cwasm` artifact or a `.tmp*` leftover.
fn cache_entry_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with(".cwasm") || name.contains(".tmp")
}
