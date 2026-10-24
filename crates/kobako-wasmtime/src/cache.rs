//! Process-wide caches for the wasmtime `Engine` and compiled
//! `Module`, plus the on-disk compiled-artifact cache.
//!
//! Wasm engine types stay hidden from the Host App and downstream
//! gems. To amortise Engine creation and
//! Module JIT compilation across multiple sandbox constructions, the
//! driver keeps a process-scope shared Engine and a per-path Module
//! cache. Both are transparent to frontends, which construct a
//! `Driver` via `Driver::new` and never see Engine or Module.
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

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

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

/// The exceptions proposal is on because the mruby wasi build emits
/// new-style exception handling (`-mllvm -wasm-use-legacy-eh=false`) for
/// its setjmp path. Epoch interruption backs the wall-clock cap, driven by
/// one process-wide ticker thread.
pub(crate) fn shared_engine() -> Result<&'static WtEngine, SetupError> {
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

/// A missing file answers `SetupError::ModuleNotBuilt`, the common state of
/// a fresh clone before `rake compile`.
pub(crate) fn cached_module(path: &Path) -> Result<WtModule, SetupError> {
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
    let engine = shared_engine()?;
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
#[cfg(unix)]
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
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    let digest = Sha256::digest(wasm_bytes);
    let mut name = String::with_capacity(96);
    for byte in digest {
        let _ = write!(name, "{:02x}", byte);
    }
    let _ = write!(name, "-{:016x}.cwasm", engine_key(engine));
    Some(base.join("kobako").join(name))
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
#[cfg(unix)]
fn load_artifact(engine: &WtEngine, artifact: &Path) -> Option<WtModule> {
    use rustix::fs::{openat, Mode, OFlags};
    let dir = open_private_dir(artifact.parent()?)?;
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let file = fs::File::from(openat(&dir, artifact.file_name()?, flags, Mode::empty()).ok()?);
    let _ = file.set_modified(std::time::SystemTime::now());
    // SAFETY: `Module::deserialize_open_file` trusts the artifact bytes
    // and needs them unchanged while the Module lives. The file was
    // opened through the directory `open_private_dir` checked, which only
    // the current user can write, so it is an artifact this module
    // stored, addressed by the content hash of the Guest Binary being
    // constructed — it carries exactly that binary's trust.
    unsafe { WtModule::deserialize_open_file(engine, file) }.ok()
}

/// The temp-file and rename pair keeps concurrent processes from observing
/// a partial write. Failures are swallowed because the cache is only an
/// optimisation, and pruning on write keeps the directory bounded across
/// rebuilds.
#[cfg(unix)]
fn store_artifact(module: &WtModule, artifact: &Path) {
    use rustix::fs::{openat, renameat, Mode, OFlags};
    use std::io::Write as _;
    let (Some(dir), Some(name)) = (artifact.parent(), artifact.file_name()) else {
        return;
    };
    let Ok(bytes) = module.serialize() else {
        return;
    };
    if create_cache_dir(dir).is_err() {
        return;
    }
    let Some(dir) = open_private_dir(dir) else {
        return;
    };
    let tmp = Path::new(name).with_extension(format!("tmp{}", std::process::id()));
    let flags =
        OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let Ok(fd) = openat(&dir, &tmp, flags, Mode::RUSR | Mode::WUSR) else {
        return;
    };
    if fs::File::from(fd).write_all(&bytes).is_err() {
        return;
    }
    if renameat(&dir, &tmp, &dir, name).is_ok() {
        prune_stale(&dir, name);
    }
}

/// The disk cache trusts its directory's owner, which only Unix reports,
/// so elsewhere every process compiles the Guest Binary itself.
#[cfg(not(unix))]
fn load_artifact(_engine: &WtEngine, _artifact: &Path) -> Option<WtModule> {
    None
}

#[cfg(not(unix))]
fn store_artifact(_module: &WtModule, _artifact: &Path) {}

/// Create the cache directory owner-only (`0700`) so no other local user
/// can plant an artifact the unsafe deserialize would trust.
#[cfg(unix)]
fn create_cache_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

/// The cache directory as one descriptor, once it upholds the trust the
/// unsafe deserialize relies on: a directory rather than a link in its
/// place, owned by the current effective user and writable by no one
/// else. Every later step goes through the descriptor, so a path swapped
/// after the check reaches nothing. One that fails — another user's,
/// under a shared `XDG_CACHE_HOME`, say — skips both disk-cache tiers.
#[cfg(unix)]
fn open_private_dir(dir: &Path) -> Option<rustix::fd::OwnedFd> {
    use rustix::fs::{fstat, open, Mode, OFlags};
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = open(dir, flags, Mode::empty()).ok()?;
    let stat = fstat(&fd).ok()?;
    let owned = stat.st_uid == rustix::process::geteuid().as_raw();
    (owned && stat.st_mode & 0o022 == 0).then_some(fd)
}

/// Live temp files are seconds old and never qualify; foreign file names
/// are left untouched.
#[cfg(unix)]
fn prune_stale(dir: &rustix::fd::OwnedFd, keep: &std::ffi::OsStr) {
    use rustix::fs::{statat, unlinkat, AtFlags, Dir};
    use std::os::unix::ffi::OsStrExt;
    use std::time::{SystemTime, UNIX_EPOCH};
    let Ok(mut entries) = Dir::read_from(dir) else {
        return;
    };
    while let Some(Ok(entry)) = entries.read() {
        let name = entry.file_name();
        if name.to_bytes() == keep.as_bytes() || !name.to_str().is_ok_and(cache_entry_name) {
            continue;
        }
        let stale = statat(dir, name, AtFlags::SYMLINK_NOFOLLOW)
            .ok()
            .and_then(|stat| u64::try_from(stat.st_mtime).ok())
            .and_then(|secs| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH + Duration::from_secs(secs))
                    .ok()
            })
            .is_some_and(|age| age > ARTIFACT_TTL);
        if stale {
            let _ = unlinkat(dir, name, AtFlags::empty());
        }
    }
}

#[cfg(unix)]
fn cache_entry_name(name: &str) -> bool {
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
