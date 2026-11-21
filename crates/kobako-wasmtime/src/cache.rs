//! Process-wide caches for the wasmtime `Engine` and compiled
//! `Module`, plus the on-disk compiled-artifact cache.
//!
//! Wasm engine types stay hidden from the Host App and downstream
//! gems. To amortise Engine creation and
//! Module JIT compilation across multiple sandbox constructions, the
//! driver keeps a process-scope shared Engine and a per-path Module
//! cache. Both are transparent to frontends, which construct a
//! `Driver` via `Driver::from_file` and never see Engine or Module.
//!
//! Across processes, the Cranelift compile cost is amortised by a
//! best-effort `.cwasm` disk cache keyed by the SHA-256 of the Guest
//! Binary bytes and the engine that compiles them; every cache failure
//! falls back to in-process compilation.
//!
//! Concurrency: the Mutex is held only for the HashMap lookup and insert,
//! never across a compile, so threads building Drivers at once do not
//! wait on each other. Two concurrent misses on one path both compile,
//! and the later insert wins with an equivalent Module.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime};

#[cfg(unix)]
use unix::PrivateDir;
#[cfg(windows)]
use windows::PrivateDir;

use sha2::{Digest, Sha256};
use wasmtime::{Config as WtConfig, Engine as WtEngine, Module as WtModule};

use kobako_runtime::error::SetupError;
use kobako_runtime::fork::ProcessLocal;

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

/// The exceptions proposal is on because the mruby wasi build emits
/// new-style exception handling (`-mllvm -wasm-use-legacy-eh=false`) for
/// its setjmp path. Epoch interruption backs the wall-clock cap, driven by
/// one ticker thread per process.
pub(crate) fn load_engine() -> Result<&'static WtEngine, SetupError> {
    if let Some(engine) = SHARED_ENGINE.get() {
        spawn_epoch_ticker(engine);
        return Ok(engine);
    }
    let mut config = WtConfig::new();
    config.wasm_exceptions(true);
    config.epoch_interruption(true);
    catch_traps(&mut config);
    let engine =
        WtEngine::new(&config).map_err(|e| SetupError::Dead(format!("engine init: {e}")))?;
    let engine = SHARED_ENGINE.get_or_init(|| engine);
    spawn_epoch_ticker(engine);
    Ok(engine)
}

/// macOS hands a guest's faults to wasmtime over a Mach port by default,
/// which no forked child can use; POSIX signals carry across a fork. The
/// choice belongs to the whole process, so every Engine in it must make the
/// same one, or wasmtime panics creating the one that differs.
fn catch_traps(config: &mut WtConfig) {
    config.macos_use_mach_ports(!cfg!(feature = "posix-signals-on-macos"));
}

/// The ticker is the process's own: a forked child, which inherits the Engine
/// but no thread, starts one of its own, or no deadline would fire in it.
fn spawn_epoch_ticker(engine: &WtEngine) {
    static TICKER: ProcessLocal<()> = ProcessLocal::new();
    TICKER.with(|ticker| {
        if ticker.is_none() {
            let engine = engine.clone();
            thread::Builder::new()
                .name("kobako-epoch-ticker".into())
                .spawn(move || loop {
                    thread::sleep(EPOCH_TICK);
                    engine.increment_epoch();
                })
                .expect("spawn kobako-epoch-ticker thread");
            *ticker = Some(());
        }
    });
}

/// A missing file answers `SetupError::GuestBinaryNotFound`, the common state of
/// a fresh clone before `rake compile`.
pub(crate) fn load_module(path: &Path) -> Result<WtModule, SetupError> {
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
        return Err(SetupError::GuestBinaryNotFound(format!(
            "Sandbox runtime not found at {}",
            path.display()
        )));
    }

    let bytes = fs::read(path).map_err(|e| {
        SetupError::Dead(format!(
            "failed to read Sandbox runtime at {}: {e}",
            path.display()
        ))
    })?;
    let engine = load_engine()?;
    let artifact = artifact_path(engine, &bytes);
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

/// Content addressing makes a rebuilt Guest Binary a new entry rather than
/// an invalidation problem. The engine segment names the wasmtime release
/// and Config that compiled the artifact, because wasmtime's own
/// deserialize check compares only the major version: without it, an
/// artifact from a release with a since-fixed miscompile would load
/// under the patched one.
fn artifact_path(engine: &WtEngine, wasm_bytes: &[u8]) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(cache_home)?;
    let digest = Sha256::digest(wasm_bytes);
    let mut name = String::with_capacity(96);
    for byte in digest {
        let _ = write!(name, "{:02x}", byte);
    }
    let _ = write!(name, "-{:016x}.cwasm", engine_key(engine));
    Some(base.join("kobako").join(name))
}

/// Where the platform keeps a user's caches when `XDG_CACHE_HOME` names
/// none: `~/.cache` on Unix, as the XDG Base Directory spec sets, and the
/// local application data folder on Windows.
#[cfg(unix)]
fn cache_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache"))
}

#[cfg(windows)]
fn cache_home() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

/// wasmtime's hash of everything an artifact's compatibility depends on,
/// its full release version included. `DefaultHasher::new` is unseeded,
/// so every process of one build derives the same key.
fn engine_key(engine: &WtEngine) -> u64 {
    let mut hasher = DefaultHasher::new();
    engine.precompile_compatibility_hash().hash(&mut hasher);
    hasher.finish()
}

/// A hit refreshes the file's mtime, so the retention window measures time
/// since last use rather than since creation.
fn load_artifact(engine: &WtEngine, artifact: &Path) -> Option<WtModule> {
    let dir = PrivateDir::open(artifact.parent()?)?;
    let file = dir.open_entry(artifact.file_name()?)?;
    let _ = file.set_modified(SystemTime::now());
    // SAFETY: `Module::deserialize_open_file` trusts the artifact bytes
    // and needs them unchanged while the Module lives. The file was
    // opened through the directory `PrivateDir::open` checked, which no
    // other user can write, so it is an artifact this module
    // stored, addressed by the content hash of the Guest Binary being
    // constructed — it carries exactly that binary's trust.
    unsafe { WtModule::deserialize_open_file(engine, file) }.ok()
}

/// The temp-file and rename pair keeps concurrent processes from observing
/// a partial write, and syncing the bytes before the rename keeps a crash
/// from leaving the artifact's name over blocks never written, which the
/// unsafe deserialize would trust. Failures are swallowed because the
/// cache is only an optimisation, and pruning on write keeps the
/// directory bounded across rebuilds.
fn store_artifact(module: &WtModule, artifact: &Path) {
    use std::io::Write as _;
    let (Some(dir), Some(name)) = (artifact.parent(), artifact.file_name()) else {
        return;
    };
    let Ok(bytes) = module.serialize() else {
        return;
    };
    if PrivateDir::create(dir).is_err() {
        return;
    }
    let Some(dir) = PrivateDir::open(dir) else {
        return;
    };
    let Some((tmp, mut file)) = dir.create_temp(name) else {
        return;
    };
    if file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        return;
    }
    if dir.rename(&tmp, name) {
        prune_stale(&dir, name);
    }
}

/// Live temp files are seconds old and never qualify; foreign file names
/// are left untouched.
fn prune_stale(dir: &PrivateDir, keep: &OsStr) {
    for (name, modified) in dir.entries() {
        if name == keep || !name.to_str().is_some_and(is_cache_entry_name) {
            continue;
        }
        let stale = SystemTime::now()
            .duration_since(modified)
            .is_ok_and(|age| age > ARTIFACT_TTL);
        if stale {
            dir.remove(&name);
        }
    }
}

fn is_cache_entry_name(name: &str) -> bool {
    name.ends_with(".cwasm") || name.contains(".tmp")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmtime::ModuleVersionStrategy;

    fn engine_with(version: &str) -> WtEngine {
        let mut config = WtConfig::new();
        config
            .module_version(ModuleVersionStrategy::Custom(version.into()))
            .expect("a short version string is accepted");
        catch_traps(&mut config);
        WtEngine::new(&config).expect("engine")
    }

    fn entry_name(engine: &WtEngine) -> PathBuf {
        let path = artifact_path(engine, b"\0asm").expect("a cache root");
        PathBuf::from(path.file_name().expect("an entry name"))
    }

    // A custom module version stands in for another wasmtime release:
    // wasmtime's compatibility hash covers its full version the same way.
    // @behavior RT-108
    #[test]
    fn an_artifact_from_another_engine_release_is_never_the_entry_read() {
        assert_ne!(
            entry_name(&engine_with("49.0.3")),
            entry_name(&engine_with("49.0.4")),
            "a Guest Binary cached by one engine release must not be the entry another release reads"
        );
    }

    // Two writers in one process take temp files in turn; each must get a
    // file of its own rather than reopen the other's.
    // @behavior RT-113
    #[test]
    fn two_writers_of_one_artifact_never_share_a_temp_file() {
        let root = std::env::temp_dir().join(format!("kobako-temp-{}", std::process::id()));
        PrivateDir::create(&root).expect("a scratch directory");
        let dir = PrivateDir::open(&root).expect("a private directory");
        let name = std::ffi::OsStr::new("entry.cwasm");
        let first = dir.create_temp(name).map(|(path, _)| path);
        let second = dir.create_temp(name).map(|(path, _)| path);
        // Let go first: Windows keeps a held directory from being removed.
        drop(dir);
        fs::remove_dir_all(&root).expect("the scratch directory removed");
        let (first, second) = (
            first.expect("a temp file"),
            second.expect("a second temp file"),
        );
        assert_ne!(
            first, second,
            "two stores of one artifact in one process must each write a temp file of their own"
        );
    }

    // @behavior RT-108
    #[test]
    fn engines_of_one_release_and_config_share_the_entry() {
        assert_eq!(
            entry_name(&engine_with("49.0.4")),
            entry_name(&engine_with("49.0.4")),
            "two engines of one release and Config must read the entry either of them cached"
        );
    }
}
