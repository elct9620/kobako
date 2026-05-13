//! Process-wide caches for the wasmtime [`Engine`] and compiled
//! [`Module`].
//!
//! SPEC.md "Code Organization" pins `ext/` as private and forbids
//! exposing wasm engine types to the Host App or downstream gems. To
//! amortise Engine creation and Module JIT compilation across multiple
//! `Kobako::Sandbox` constructions, the ext keeps a process-scope
//! shared Engine and a per-path Module cache. Both are transparent to
//! Ruby callers, who construct an `Instance` via
//! `Kobako::Wasm::Instance.from_path(...)` and never see Engine or
//! Module.
//!
//! Concurrency: under Ruby's GVL only one thread can execute Rust code
//! at a time, so the Mutex is held briefly during HashMap insert/lookup
//! and serves to satisfy `Sync` bounds rather than to arbitrate real
//! contention.
//!
//! [`Engine`]: wasmtime::Engine
//! [`Module`]: wasmtime::Module

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use magnus::{Error as MagnusError, Ruby};
use wasmtime::{Config as WtConfig, Engine as WtEngine, Module as WtModule};

use super::{wasm_err, MODULE_NOT_BUILT_ERROR};

static SHARED_ENGINE: OnceLock<WtEngine> = OnceLock::new();
static MODULE_CACHE: OnceLock<Mutex<HashMap<PathBuf, WtModule>>> = OnceLock::new();

/// Return the process-wide wasmtime Engine, building it on first call.
///
/// Enables the wasm exceptions proposal so `kobako.wasm` (which uses
/// `try_table` / `exnref` / `tag` for mruby's setjmp-via-new-EH path)
/// can be loaded. The mruby wasi build config uses
/// `-mllvm -wasm-use-legacy-eh=false`, which generates new-style
/// exception handling instructions in the wasm32 object files;
/// wasmtime must have the proposal enabled to parse and JIT those
/// instructions.
pub(crate) fn shared_engine() -> Result<&'static WtEngine, MagnusError> {
    if let Some(engine) = SHARED_ENGINE.get() {
        return Ok(engine);
    }
    let mut config = WtConfig::new();
    config.wasm_exceptions(true);
    let engine = WtEngine::new(&config).map_err(|e| {
        let ruby = Ruby::get().expect("Ruby thread");
        wasm_err(&ruby, format!("engine init: {}", e))
    })?;
    Ok(SHARED_ENGINE.get_or_init(|| engine))
}

/// Look up `path` in the per-path Module cache, compiling and inserting
/// the artifact on a miss. Raises `Kobako::Wasm::ModuleNotBuiltError`
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
                "wasm module not found at {}; run `bundle exec rake wasm:build` to build it",
                path.display()
            ),
        ));
    }

    let bytes =
        fs::read(path).map_err(|e| wasm_err(&ruby, format!("read {}: {}", path.display(), e)))?;
    let module = WtModule::new(shared_engine()?, &bytes)
        .map_err(|e| wasm_err(&ruby, format!("compile module: {}", e)))?;
    cache
        .lock()
        .expect("module cache mutex poisoned")
        .insert(path.to_path_buf(), module.clone());
    Ok(module)
}
