// Host-side wasmtime wrapper. Exposes a minimal binding surface to Ruby
// per tmp/REFERENCE.md Ch.6 §wasmtime crate 最小 binding 範圍:
//
//   Kobako::Wasm::Engine     - wraps wasmtime::Engine
//   Kobako::Wasm::Module     - wraps wasmtime::Module (file-loaded)
//   Kobako::Wasm::Store      - wraps wasmtime::Store<HostState>
//   Kobako::Wasm::Instance   - wraps wasmtime::Instance + cached TypedFuncs
//
// This is the foundational binding layer for items #14 (Sandbox), #16
// (run path) and #18 (RPC dispatch). The `__kobako_rpc_call` host import
// is wired here as a stub that records (req_bytes, return_bytes) pairs in
// HostState; later items replace the body with real Registry dispatch.

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use magnus::{
    function, method, prelude::*, value::Lazy, Error as MagnusError, ExceptionClass, RModule, Ruby,
};
use wasmtime::{
    AsContextMut, Caller, Engine as WtEngine, Extern, Instance as WtInstance, Linker,
    Module as WtModule, Store as WtStore, TypedFunc,
};

// ---------------------------------------------------------------------------
// Error classes (lazy-resolved from Ruby once Kobako::Wasm is defined)
// ---------------------------------------------------------------------------

static MODULE_NOT_BUILT_ERROR: Lazy<ExceptionClass> = Lazy::new(|ruby| {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    let wasm: RModule = kobako.const_get("Wasm").unwrap();
    wasm.const_get("ModuleNotBuiltError").unwrap()
});

static WASM_ERROR: Lazy<ExceptionClass> = Lazy::new(|ruby| {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    let wasm: RModule = kobako.const_get("Wasm").unwrap();
    wasm.const_get("Error").unwrap()
});

fn wasm_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    MagnusError::new(ruby.get_inner(&WASM_ERROR), msg.into())
}

// ---------------------------------------------------------------------------
// HostState — context carried inside wasmtime::Store<T>.
// ---------------------------------------------------------------------------

/// Per-Store host state. Item #12 only needs placeholder buffers and an
/// import-call recorder; real wiring (Registry handle, outcome decoding)
/// arrives in items #14/#16/#18.
#[derive(Default)]
#[allow(dead_code)] // Buffers reserved for items #16/#18; populated then.
pub struct HostState {
    /// Buffer mirror of guest's OUTCOME_BUFFER. Filled by `__kobako_take_outcome`
    /// post-execution in #16; unused at #12 but reserved here so signatures stay
    /// stable.
    pub outcome: Vec<u8>,
    /// stdout/stderr collectors (item #16 wires WASI here).
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Recorded `__kobako_rpc_call` invocations from the stub import.
    /// Each entry is (request_bytes_received, response_bytes_returned).
    pub rpc_calls: Vec<(Vec<u8>, Vec<u8>)>,
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

#[magnus::wrap(class = "Kobako::Wasm::Engine", free_immediately, size)]
pub struct Engine {
    inner: WtEngine,
}

impl Engine {
    fn new() -> Result<Self, MagnusError> {
        // Default Config; wasm_exceptions etc. tuned in later items.
        let engine = WtEngine::default();
        Ok(Self { inner: engine })
    }

    pub(crate) fn raw(&self) -> &WtEngine {
        &self.inner
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

#[magnus::wrap(class = "Kobako::Wasm::Module", free_immediately, size)]
pub struct Module {
    inner: WtModule,
}

impl Module {
    /// Load a wasm module from disk. Raises `Kobako::Wasm::ModuleNotBuiltError`
    /// when the file is missing — the typical case before `rake wasm:guest`
    /// has produced `data/kobako.wasm`.
    fn from_file(engine: &Engine, path: String) -> Result<Self, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let p = PathBuf::from(&path);
        if !p.exists() {
            return Err(MagnusError::new(
                ruby.get_inner(&MODULE_NOT_BUILT_ERROR),
                format!(
                    "wasm module not found at {}; run `bundle exec rake wasm:guest` to build it",
                    path
                ),
            ));
        }
        let bytes = fs::read(&p).map_err(|e| wasm_err(&ruby, format!("read {}: {}", path, e)))?;
        let module = WtModule::new(engine.raw(), &bytes)
            .map_err(|e| wasm_err(&ruby, format!("compile module: {}", e)))?;
        Ok(Self { inner: module })
    }

    pub(crate) fn raw(&self) -> &WtModule {
        &self.inner
    }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// Magnus requires `Send + Sync` for wrapped types. wasmtime::Store is not
/// Sync, so we wrap it in an `Arc<Mutex>`-equivalent: a single-threaded
/// `RefCell` is sufficient because magnus enforces single-threaded GVL
/// access from Ruby. We assert Send+Sync via an unsafe wrapper.
pub struct StoreCell(RefCell<WtStore<HostState>>);

// SAFETY: Ruby's GVL serialises access to magnus-wrapped objects on a single
// OS thread at a time. wasmtime::Store is Send (verified upstream); our
// RefCell-mediated mutation is therefore safe under the GVL invariant.
unsafe impl Send for StoreCell {}
unsafe impl Sync for StoreCell {}

#[magnus::wrap(class = "Kobako::Wasm::Store", free_immediately, size)]
pub struct Store {
    cell: Arc<StoreCell>,
}

impl Store {
    fn new(engine: &Engine) -> Result<Self, MagnusError> {
        let store = WtStore::new(engine.raw(), HostState::default());
        Ok(Self {
            cell: Arc::new(StoreCell(RefCell::new(store))),
        })
    }

    pub(crate) fn cell(&self) -> Arc<StoreCell> {
        self.cell.clone()
    }

    /// Read-only snapshot of recorded RPC stub calls — used by tests at #12,
    /// replaced by Registry dispatch at #18.
    fn rpc_call_count(&self) -> usize {
        self.cell.0.borrow().data().rpc_calls.len()
    }
}

// ---------------------------------------------------------------------------
// Instance
// ---------------------------------------------------------------------------

#[magnus::wrap(class = "Kobako::Wasm::Instance", free_immediately, size)]
pub struct Instance {
    inner: WtInstance,
    store: Arc<StoreCell>,
    // Cached TypedFunc handles for the three guest exports. Optional because
    // test fixtures (a minimal "ping" module) need not provide them; real
    // kobako.wasm always does, and #16 enforces presence at run time.
    run: Option<TypedFunc<(), ()>>,
    take_outcome: Option<TypedFunc<(), (i32, i32)>>,
    alloc: Option<TypedFunc<i32, i32>>,
}

impl Instance {
    fn new(engine: &Engine, module: &Module, store: &Store) -> Result<Self, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let mut linker: Linker<HostState> = Linker::new(engine.raw());

        // Stub `__kobako_rpc_call` host import. Signature per Ch.4 §Wire ABI:
        //   (req_ptr: i32, req_len: i32) -> i64
        // At #12 we record the call in HostState and return 0 (no response).
        // #18 replaces the body with real Registry dispatch.
        linker
            .func_wrap(
                "kobako",
                "__kobako_rpc_call",
                |mut caller: Caller<'_, HostState>, req_ptr: i32, req_len: i32| -> i64 {
                    let req_bytes = read_memory(&mut caller, req_ptr, req_len).unwrap_or_default();
                    caller.data_mut().rpc_calls.push((req_bytes, Vec::new()));
                    0_i64
                },
            )
            .map_err(|e| wasm_err(&ruby, format!("define __kobako_rpc_call: {}", e)))?;

        let cell = store.cell();
        let instance = {
            let mut store_ref = cell.0.borrow_mut();
            linker
                .instantiate(store_ref.as_context_mut(), module.raw())
                .map_err(|e| wasm_err(&ruby, format!("instantiate: {}", e)))?
        };

        // Best-effort export lookup. Missing exports are not an error here
        // (test fixture is a bare module); #16 will assert their presence
        // before invocation.
        let (run, take_outcome, alloc) = {
            let mut store_ref = cell.0.borrow_mut();
            let mut ctx = store_ref.as_context_mut();
            let run = instance
                .get_typed_func::<(), ()>(&mut ctx, "__kobako_run")
                .ok();
            let take_outcome = instance
                .get_typed_func::<(), (i32, i32)>(&mut ctx, "__kobako_take_outcome")
                .ok();
            let alloc = instance
                .get_typed_func::<i32, i32>(&mut ctx, "__kobako_alloc")
                .ok();
            (run, take_outcome, alloc)
        };

        Ok(Self {
            inner: instance,
            store: cell,
            run,
            take_outcome,
            alloc,
        })
    }

    fn has_export(&self, name: String) -> bool {
        let mut store_ref = self.store.0.borrow_mut();
        self.inner
            .get_export(store_ref.as_context_mut(), &name)
            .is_some()
    }

    /// Returns the count of cached well-known exports actually found in the
    /// instance (out of __kobako_run / __kobako_take_outcome / __kobako_alloc).
    /// Used by the real-tier E2E test to assert the full guest binary
    /// surface is intact.
    fn known_export_count(&self) -> usize {
        [self.run.is_some(), self.take_outcome.is_some(), self.alloc.is_some()]
            .iter()
            .filter(|b| **b)
            .count()
    }
}

fn read_memory(
    caller: &mut Caller<'_, HostState>,
    ptr: i32,
    len: i32,
) -> Option<Vec<u8>> {
    let mem = match caller.get_export("memory") {
        Some(Extern::Memory(m)) => m,
        _ => return None,
    };
    let data = mem.data(&caller);
    let start = ptr as usize;
    let end = start.checked_add(len as usize)?;
    data.get(start..end).map(|s| s.to_vec())
}

// ---------------------------------------------------------------------------
// Ruby init
// ---------------------------------------------------------------------------

pub fn init(ruby: &Ruby, kobako: RModule) -> Result<(), MagnusError> {
    let wasm = kobako.define_module("Wasm")?;

    // Error hierarchy. ModuleNotBuiltError is the headline error for the
    // common pre-build state (see Ch.6 §runtime 讀取策略).
    let base_err = wasm.define_error("Error", ruby.exception_standard_error())?;
    wasm.define_error("ModuleNotBuiltError", base_err)?;

    let engine = wasm.define_class("Engine", ruby.class_object())?;
    engine.define_singleton_method("new", function!(Engine::new, 0))?;

    let module = wasm.define_class("Module", ruby.class_object())?;
    module.define_singleton_method("from_file", function!(Module::from_file, 2))?;

    let store = wasm.define_class("Store", ruby.class_object())?;
    store.define_singleton_method("new", function!(Store::new, 1))?;
    store.define_method("rpc_call_count", method!(Store::rpc_call_count, 0))?;

    let instance = wasm.define_class("Instance", ruby.class_object())?;
    instance.define_singleton_method("new", function!(Instance::new, 3))?;
    instance.define_method("has_export?", method!(Instance::has_export, 1))?;
    instance.define_method("known_export_count", method!(Instance::known_export_count, 0))?;

    Ok(())
}
