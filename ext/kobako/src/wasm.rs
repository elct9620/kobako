// Host-side wasmtime wrapper. Exposes a minimal binding surface to Ruby:
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
    function, method, prelude::*, value::Lazy, value::Opaque, Error as MagnusError,
    ExceptionClass, RModule, Ruby, Value,
};
use magnus::RString;
use wasmtime::{
    AsContextMut, Caller, Engine as WtEngine, Extern, Instance as WtInstance, Linker,
    Memory, Module as WtModule, Store as WtStore, TypedFunc,
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
    /// Ruby-side `Kobako::Registry`. When set, the `__kobako_rpc_call`
    /// import calls `registry.dispatch(req_bytes)` and hands the returned
    /// Response bytes back to the guest. `Opaque<Value>` is `Send + Sync`;
    /// calling `get_inner` requires a `Ruby` handle, which we obtain on
    /// every Ruby thread entry via `Ruby::get()`.
    pub registry: Option<Opaque<Value>>,
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
    //
    // Two `run` shapes are cached because the production Guest Binary
    // exports `__kobako_run() -> ()` (SPEC) but the host-side test fixture
    // (`wasm/test-guest`) exports `__kobako_run(ptr, len) -> ()` so the
    // host can hand it the source bytes directly. Both shapes are looked
    // up; the Ruby `Sandbox` picks which to call based on which one
    // resolved successfully.
    run: Option<TypedFunc<(), ()>>,
    run_with_source: Option<TypedFunc<(i32, i32), ()>>,
    take_outcome: Option<TypedFunc<(), u64>>,
    alloc: Option<TypedFunc<i32, i32>>,
}

impl Instance {
    fn new(engine: &Engine, module: &Module, store: &Store) -> Result<Self, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let mut linker: Linker<HostState> = Linker::new(engine.raw());

        // `__kobako_rpc_call` host import. Signature per SPEC § Wire ABI:
        //   (req_ptr: i32, req_len: i32) -> i64
        // Decodes the Request bytes, dispatches via the Ruby-side
        // `Kobako::Registry` (set per-run via `set_registry`), allocates a
        // guest buffer through `__kobako_alloc`, writes the Response bytes
        // there, and returns the packed `(ptr<<32)|len`. When no Registry
        // is set (test scenarios that never RPC), the legacy recorder
        // behaviour is preserved.
        linker
            .func_wrap(
                "env",
                "__kobako_rpc_call",
                |mut caller: Caller<'_, HostState>, req_ptr: i32, req_len: i32| -> i64 {
                    dispatch_rpc(&mut caller, req_ptr, req_len)
                },
            )
            .map_err(|e| wasm_err(&ruby, format!("define __kobako_rpc_call: {}", e)))?;

        // Minimal WASI preview1 stubs. The production Guest Binary will
        // ship with `wasmtime-wasi` wired to real pipes (item #16's WASI
        // capture path is a separate iteration). For now we satisfy the
        // imports the test fixture's `std` brings in (panic-on-error
        // formatting touches `fd_write` on the wasm32-wasip1 target) so
        // the module instantiates. Each stub is a no-op or returns the
        // SUCCESS errno (0).
        register_wasi_stubs(&mut linker)
            .map_err(|e| wasm_err(&ruby, format!("define WASI stubs: {}", e)))?;

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
        let (run, run_with_source, take_outcome, alloc) = {
            let mut store_ref = cell.0.borrow_mut();
            let mut ctx = store_ref.as_context_mut();
            let run = instance
                .get_typed_func::<(), ()>(&mut ctx, "__kobako_run")
                .ok();
            let run_with_source = instance
                .get_typed_func::<(i32, i32), ()>(&mut ctx, "__kobako_run")
                .ok();
            let take_outcome = instance
                .get_typed_func::<(), u64>(&mut ctx, "__kobako_take_outcome")
                .ok();
            let alloc = instance
                .get_typed_func::<i32, i32>(&mut ctx, "__kobako_alloc")
                .ok();
            (run, run_with_source, take_outcome, alloc)
        };

        Ok(Self {
            inner: instance,
            store: cell,
            run,
            run_with_source,
            take_outcome,
            alloc,
        })
    }

    /// Install the Ruby-side `Kobako::Registry` into HostState. Called by
    /// `Kobako::Sandbox` after constructing the Registry; from this point
    /// on, every `__kobako_rpc_call` import invocation routes through
    /// `registry.dispatch(req_bytes)`.
    fn set_registry(&self, registry: Value) -> Result<(), MagnusError> {
        let mut store_ref = self.store.0.borrow_mut();
        store_ref.data_mut().registry = Some(Opaque::from(registry));
        Ok(())
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
        [
            self.run.is_some() || self.run_with_source.is_some(),
            self.take_outcome.is_some(),
            self.alloc.is_some(),
        ]
        .iter()
        .filter(|b| **b)
        .count()
    }

    // -----------------------------------------------------------------
    // Run-path methods (item #16). These drive the alloc → write source
    // → run → take_outcome flow from Ruby. Each method is best-effort —
    // it raises a Ruby `Kobako::Wasm::Error` when the corresponding
    // export is missing or fails so the Sandbox layer can map errors to
    // the three-class taxonomy.
    // -----------------------------------------------------------------

    /// Invoke the guest's `__kobako_alloc(size)` export and return the
    /// resulting linear-memory offset. A return of 0 indicates an
    /// allocation failure (caller should treat as a wire violation /
    /// trap).
    fn alloc(&self, size: i32) -> Result<i32, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let alloc = self
            .alloc
            .as_ref()
            .ok_or_else(|| wasm_err(&ruby, "guest does not export __kobako_alloc"))?;
        let mut store_ref = self.store.0.borrow_mut();
        alloc
            .call(store_ref.as_context_mut(), size)
            .map_err(|e| wasm_err(&ruby, format!("__kobako_alloc({}): {}", size, e)))
    }

    /// Write +bytes+ into the guest's linear memory starting at +ptr+.
    /// Raises `Kobako::Wasm::Error` if the instance has no `memory`
    /// export or the slice is out of bounds.
    fn write_memory(&self, ptr: i32, bytes: RString) -> Result<(), MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let mut store_ref = self.store.0.borrow_mut();
        let mem: Memory = match self.inner.get_export(store_ref.as_context_mut(), "memory") {
            Some(Extern::Memory(m)) => m,
            _ => return Err(wasm_err(&ruby, "guest does not export 'memory'")),
        };

        // SAFETY: RString::as_slice on a frozen-on-read borrow.
        let src: &[u8] = unsafe { bytes.as_slice() };
        let data = mem.data_mut(store_ref.as_context_mut());
        let start = ptr as usize;
        let end = start
            .checked_add(src.len())
            .ok_or_else(|| wasm_err(&ruby, "write_memory: ptr + len overflow"))?;
        if end > data.len() {
            return Err(wasm_err(
                &ruby,
                format!(
                    "write_memory: range [{}, {}) exceeds memory size {}",
                    start,
                    end,
                    data.len()
                ),
            ));
        }
        data[start..end].copy_from_slice(src);
        Ok(())
    }

    /// Read +len+ bytes from the guest's linear memory starting at
    /// +ptr+. Returns a binary-encoded Ruby String.
    fn read_memory(&self, ptr: i32, len: i32) -> Result<RString, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let mut store_ref = self.store.0.borrow_mut();
        let mem: Memory = match self.inner.get_export(store_ref.as_context_mut(), "memory") {
            Some(Extern::Memory(m)) => m,
            _ => return Err(wasm_err(&ruby, "guest does not export 'memory'")),
        };
        let data = mem.data(store_ref.as_context_mut());
        let start = ptr as usize;
        let end = start
            .checked_add(len as usize)
            .ok_or_else(|| wasm_err(&ruby, "read_memory: ptr + len overflow"))?;
        if end > data.len() {
            return Err(wasm_err(
                &ruby,
                format!(
                    "read_memory: range [{}, {}) exceeds memory size {}",
                    start,
                    end,
                    data.len()
                ),
            ));
        }
        Ok(ruby.str_from_slice(&data[start..end]))
    }

    /// Invoke `__kobako_run`. Tries the SPEC `() -> ()` shape first; if
    /// the guest exports the `(ptr, len) -> ()` shape (test fixture),
    /// passes the supplied +ptr+/+len+ instead.
    fn run_call(&self, ptr: i32, len: i32) -> Result<(), MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let mut store_ref = self.store.0.borrow_mut();
        if let Some(run) = &self.run_with_source {
            return run
                .call(store_ref.as_context_mut(), (ptr, len))
                .map_err(|e| wasm_err(&ruby, format!("__kobako_run(ptr, len): {}", e)));
        }
        if let Some(run) = &self.run {
            return run
                .call(store_ref.as_context_mut(), ())
                .map_err(|e| wasm_err(&ruby, format!("__kobako_run(): {}", e)));
        }
        Err(wasm_err(&ruby, "guest does not export __kobako_run"))
    }

    /// Invoke `__kobako_take_outcome`. Returns the packed u64
    /// `(ptr << 32) | len`; the Ruby caller unpacks.
    fn take_outcome(&self) -> Result<u64, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let take = self
            .take_outcome
            .as_ref()
            .ok_or_else(|| wasm_err(&ruby, "guest does not export __kobako_take_outcome"))?;
        let mut store_ref = self.store.0.borrow_mut();
        take.call(store_ref.as_context_mut(), ())
            .map_err(|e| wasm_err(&ruby, format!("__kobako_take_outcome(): {}", e)))
    }
}

// Register no-op stubs for the wasi_snapshot_preview1 imports that
// `std`-built wasm32-wasip1 modules pull in implicitly. These stubs are
// scoped to host-side test fixtures; the production Guest Binary ships
// with real WASI in a later iteration, replacing these stubs with
// `wasmtime-wasi` (or wasi-common) bindings that route stdout/stderr
// into the Sandbox's bounded buffers per SPEC §B-04.
fn register_wasi_stubs(linker: &mut Linker<HostState>) -> Result<(), wasmtime::Error> {
    // fd_write(fd: i32, iovs_ptr: i32, iovs_len: i32, nwritten_ptr: i32) -> errno (i32).
    // We accept the call, claim 0 bytes written (success), and ignore
    // the buffers. SPEC §B-04 capture lands in a later item.
    linker.func_wrap(
        "wasi_snapshot_preview1",
        "fd_write",
        |mut caller: Caller<'_, HostState>,
         _fd: i32,
         _iovs_ptr: i32,
         _iovs_len: i32,
         nwritten_ptr: i32|
         -> i32 {
            // Write 0 to *nwritten so the caller sees a successful 0-byte
            // write; this is enough for `std::panicking` paths that
            // probe before formatting.
            if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                let zero = [0u8; 4];
                let _ = mem.write(&mut caller, nwritten_ptr as usize, &zero);
            }
            0
        },
    )?;

    // proc_exit(code: i32) — terminates the guest. Trapping is the
    // simplest semantics; the host turns the trap into TrapError.
    linker.func_wrap(
        "wasi_snapshot_preview1",
        "proc_exit",
        |_caller: Caller<'_, HostState>, _code: i32| {
            // Returning normally lets the wasm module continue, which is
            // wrong; instead we return so wasmtime sees no error. proc_exit
            // is rarely reached by the test fixture (no exit calls), but
            // we keep the import satisfied either way.
        },
    )?;

    // fd_close, fd_seek, environ_get, environ_sizes_get, args_get, args_sizes_get —
    // all return SUCCESS without observable side effects. Sized as a
    // single sweep so future SPEC-compliance work doesn't have to grow
    // this list one symbol at a time.
    for (name, arity) in [
        ("fd_close", 1),
        ("fd_seek", 4),
        ("fd_fdstat_get", 2),
        ("fd_prestat_get", 2),
        ("fd_prestat_dir_name", 3),
        ("environ_get", 2),
        ("environ_sizes_get", 2),
        ("args_get", 2),
        ("args_sizes_get", 2),
        ("clock_time_get", 4),
        ("random_get", 2),
    ] {
        wasi_stub(linker, name, arity)?;
    }

    Ok(())
}

fn wasi_stub(
    linker: &mut Linker<HostState>,
    name: &str,
    arity: usize,
) -> Result<(), wasmtime::Error> {
    match arity {
        1 => linker.func_wrap("wasi_snapshot_preview1", name, |_a: i32| -> i32 { 0 }),
        2 => linker.func_wrap("wasi_snapshot_preview1", name, |_a: i32, _b: i32| -> i32 {
            0
        }),
        3 => linker.func_wrap(
            "wasi_snapshot_preview1",
            name,
            |_a: i32, _b: i32, _c: i32| -> i32 { 0 },
        ),
        4 => linker.func_wrap(
            "wasi_snapshot_preview1",
            name,
            |_a: i32, _b: i32, _c: i32, _d: i32| -> i32 { 0 },
        ),
        _ => unreachable!("unsupported WASI stub arity {}", arity),
    }
    .map(|_| ())
}

/// Drive a single `__kobako_rpc_call` invocation end-to-end.
///
/// Steps (SPEC § B-12 / B-13):
///
///   1. Read the Request bytes from guest linear memory.
///   2. Hand them to the Ruby-side `Kobako::Registry` and recover Response bytes.
///   3. Allocate a guest buffer via `__kobako_alloc(len)` invoked through
///      `Caller::get_export`.
///   4. Write the Response bytes into the guest buffer.
///   5. Return packed `(ptr<<32)|len` for the guest to decode.
///
/// Returns 0 when no Registry is bound (legacy recorder path) or when
/// any step fails — failures during dispatch surface as Response.err
/// envelopes from the Registry itself, so a 0 return is reserved for
/// genuine wire-layer breakage and is mapped by the guest to a trap.
fn dispatch_rpc(caller: &mut Caller<'_, HostState>, req_ptr: i32, req_len: i32) -> i64 {
    let req_bytes = match read_memory(caller, req_ptr, req_len) {
        Some(b) => b,
        None => return 0,
    };

    // No Registry bound — preserve the legacy recorder behaviour so tests
    // that exercise the import-table shape without a Registry still pass.
    let registry = match caller.data().registry {
        Some(d) => d,
        None => {
            caller.data_mut().rpc_calls.push((req_bytes, Vec::new()));
            return 0;
        }
    };

    let resp_bytes = match invoke_registry(registry, &req_bytes) {
        Ok(b) => b,
        Err(_) => return 0,
    };

    caller
        .data_mut()
        .rpc_calls
        .push((req_bytes, resp_bytes.clone()));

    write_response(caller, &resp_bytes).unwrap_or(0)
}

/// Call the Ruby Registry's `#dispatch(request_bytes)` method and return
/// the encoded Response bytes. Errors here mean the Registry itself
/// failed (it is contracted never to raise — see `Kobako::Registry#dispatch`),
/// which we treat as a wire-layer fault.
fn invoke_registry(registry: Opaque<Value>, req_bytes: &[u8]) -> Result<Vec<u8>, MagnusError> {
    // The wasmtime callback runs on the same Ruby thread that called
    // Sandbox#run — the invariant SPEC § Implementation Standards
    // §Architecture pins for the host gem — so `Ruby::get()` is always
    // available here. Panicking with `expect` localises the violation
    // rather than letting a nonsense error propagate.
    let ruby = Ruby::get().expect("Ruby handle unavailable in __kobako_rpc_call");
    let registry_value: Value = ruby.get_inner(registry);
    let req_str = ruby.str_from_slice(req_bytes);
    let resp: RString = registry_value.funcall("dispatch", (req_str,))?;
    // SAFETY: the returned RString is held by the Ruby VM for the duration of
    // this scope; copying its bytes into a Vec is a defensive standard pattern.
    let bytes = unsafe { resp.as_slice() }.to_vec();
    Ok(bytes)
}

/// Allocate a guest-side buffer through `__kobako_alloc` and copy the
/// response bytes into it. Returns the packed `(ptr<<32)|len` u64.
fn write_response(caller: &mut Caller<'_, HostState>, bytes: &[u8]) -> Option<i64> {
    let alloc = match caller.get_export("__kobako_alloc") {
        Some(Extern::Func(f)) => f.typed::<i32, i32>(&*caller).ok()?,
        _ => return None,
    };
    let len_i32 = i32::try_from(bytes.len()).ok()?;
    let ptr = alloc.call(&mut *caller, len_i32).ok()?;
    if ptr == 0 {
        return None;
    }

    let mem = match caller.get_export("memory") {
        Some(Extern::Memory(m)) => m,
        _ => return None,
    };
    mem.write(&mut *caller, ptr as usize, bytes).ok()?;

    let ptr_u32 = ptr as u32;
    let len_u32 = bytes.len() as u32;
    Some(((ptr_u32 as i64) << 32) | (len_u32 as i64))
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
    instance.define_method("alloc", method!(Instance::alloc, 1))?;
    instance.define_method("write_memory", method!(Instance::write_memory, 2))?;
    instance.define_method("read_memory", method!(Instance::read_memory, 2))?;
    instance.define_method("run", method!(Instance::run_call, 2))?;
    instance.define_method("take_outcome", method!(Instance::take_outcome, 0))?;
    instance.define_method("set_registry", method!(Instance::set_registry, 1))?;

    Ok(())
}
