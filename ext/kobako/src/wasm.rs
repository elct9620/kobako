// Host-side wasmtime wrapper. Exposes a minimal binding surface to Ruby:
//
//   Kobako::Wasm::Engine     - wraps wasmtime::Engine
//   Kobako::Wasm::Module     - wraps wasmtime::Module (file-loaded)
//   Kobako::Wasm::Store      - wraps wasmtime::Store<HostState>
//   Kobako::Wasm::Instance   - wraps wasmtime::Instance + cached TypedFuncs
//
// WASI stdout/stderr capture (SPEC.md B-04): wasmtime-wasi p1 bindings route
// guest fd 1 and fd 2 into per-run MemoryOutputPipe instances. After each run
// the host drains the pipes via Instance#take_stdout / #take_stderr and pushes
// the raw bytes through Ruby's OutputBuffer (which enforces the cap and
// `[truncated]` marker). Stdin carries the two-frame length-prefixed protocol:
// Frame 1 (preamble msgpack) followed by Frame 2 (user script UTF-8), each
// prefixed by a 4-byte big-endian u32 length. Written via `setup_wasi_frames`
// before each run (SPEC.md ABI Signatures).

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use magnus::RString;
use magnus::{
    function, method, prelude::*, value::Lazy, value::Opaque, Error as MagnusError, ExceptionClass,
    RModule, Ruby, Value,
};
use wasmtime::{
    AsContextMut, Caller, Config as WtConfig, Engine as WtEngine, Extern, Instance as WtInstance,
    Linker, Memory, Module as WtModule, Store as WtStore, TypedFunc,
};
use wasmtime_wasi::p1;
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::WasiCtxBuilder;

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

/// Per-Store host state threaded through every host import callback.
///
/// WASI p1 state is embedded as `Option<WasiP1Ctx>` so it can be replaced
/// fresh before each `#run` without rebuilding the Store. The `stdout_pipe`
/// and `stderr_pipe` clones are kept alongside so the Ruby layer can drain
/// captured bytes after execution without touching the WASI internals.
#[derive(Default)]
pub struct HostState {
    /// Buffer mirror of guest's OUTCOME_BUFFER. Filled by `__kobako_take_outcome`
    /// post-execution. Reserved for a future streaming-outcome path; not yet
    /// consumed on the Rust side (the Ruby layer reads outcome via read_memory).
    #[allow(dead_code)]
    pub outcome: Vec<u8>,
    /// WASI p1 context for the current (or most-recent) run. Replaced before
    /// each `#run` so stdin/stdout/stderr pipes are always fresh (SPEC.md B-03).
    pub wasi: Option<WasiP1Ctx>,
    /// Clone of the MemoryOutputPipe wired to guest fd 1 (stdout). Retained
    /// here so `take_stdout` can call `contents()` after execution without
    /// having to dig into the WASI ctx internals.
    pub stdout_pipe: Option<MemoryOutputPipe>,
    /// Clone of the MemoryOutputPipe wired to guest fd 2 (stderr).
    pub stderr_pipe: Option<MemoryOutputPipe>,
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
        // Enable the wasm exceptions proposal so kobako.wasm (which uses
        // try_table / exnref / tag for mruby's setjmp-via-new-EH path) can
        // be loaded. The mruby wasi build config uses
        //   -mllvm -wasm-use-legacy-eh=false
        // which generates new-style exception handling instructions in the
        // wasm32 object files; wasmtime must have the proposal enabled to
        // parse and JIT those instructions.
        let mut config = WtConfig::new();
        config.wasm_exceptions(true);
        let engine = WtEngine::new(&config).map_err(|e| {
            let ruby = Ruby::get().expect("Ruby thread");
            wasm_err(&ruby, format!("engine init: {}", e))
        })?;
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
    // kobako.wasm always does, and the host enforces presence at run time.
    //
    // Exactly the SPEC ABI shape: `__kobako_run() -> ()`. Source delivery
    // uses the WASI stdin two-frame protocol (see `setup_wasi_frames`).
    run: Option<TypedFunc<(), ()>>,
    take_outcome: Option<TypedFunc<(), u64>>,
    alloc: Option<TypedFunc<i32, i32>>,
}

impl Instance {
    fn new(engine: &Engine, module: &Module, store: &Store) -> Result<Self, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let mut linker: Linker<HostState> = Linker::new(engine.raw());

        // Wire the wasmtime-wasi preview1 WASI imports. This replaces the
        // manual no-op stubs and routes guest fd 1/2 to the MemoryOutputPipes
        // set up before each run via `setup_wasi_pipes`. The closure extracts
        // `&mut WasiP1Ctx` from HostState; if none is set (e.g. a test module
        // that never invokes WASI functions), the Option unwrap will panic —
        // but `setup_wasi_pipes` is always called before any WASI-enabled run.
        p1::add_to_linker_sync(&mut linker, |state: &mut HostState| {
            state
                .wasi
                .as_mut()
                .expect("WASI context not initialised — call setup_wasi_pipes before run")
        })
        .map_err(|e| wasm_err(&ruby, format!("add WASI p1 to linker: {}", e)))?;

        // `__kobako_rpc_call` host import. Signature per SPEC Wire ABI:
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

        let cell = store.cell();
        let instance = {
            let mut store_ref = cell.0.borrow_mut();
            linker
                .instantiate(store_ref.as_context_mut(), module.raw())
                .map_err(|e| wasm_err(&ruby, format!("instantiate: {}", e)))?
        };

        // Best-effort export lookup. Missing exports are not an error here
        // (test fixture is a bare module); the host enforces presence before
        // invocation. Only the SPEC ABI `() -> ()` shape is accepted for
        // `__kobako_run` — the transitional `(ptr, len) -> ()` shape is gone.
        let (run, take_outcome, alloc) = {
            let mut store_ref = cell.0.borrow_mut();
            let mut ctx = store_ref.as_context_mut();
            let run = instance
                .get_typed_func::<(), ()>(&mut ctx, "__kobako_run")
                .ok();
            let take_outcome = instance
                .get_typed_func::<(), u64>(&mut ctx, "__kobako_take_outcome")
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
            self.run.is_some(),
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

    /// Invoke `__kobako_run` with the SPEC `() -> ()` shape. Source is
    /// delivered via the WASI stdin two-frame protocol written by
    /// `setup_wasi_frames` before this call.
    fn run_call(&self) -> Result<(), MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let run = self
            .run
            .as_ref()
            .ok_or_else(|| wasm_err(&ruby, "guest does not export __kobako_run"))?;
        let mut store_ref = self.store.0.borrow_mut();
        run.call(store_ref.as_context_mut(), ())
            .map_err(|e| wasm_err(&ruby, format!("__kobako_run(): {}", e)))
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

    // -----------------------------------------------------------------
    // WASI capture path (SPEC.md B-04). Called by Ruby's Sandbox#run.
    // -----------------------------------------------------------------

    /// Initialise fresh WASI pipes with the two-frame stdin content.
    ///
    /// Must be called before each `run` invocation. Creates:
    ///   * A MemoryInputPipe for stdin with Frame 1 (preamble) + Frame 2
    ///     (user script) encoded as length-prefixed frames: each frame is a
    ///     4-byte big-endian u32 length prefix followed by the payload bytes.
    ///     The guest reads both frames from WASI stdin (SPEC.md ABI Signatures).
    ///   * A MemoryOutputPipe for fd 1 (stdout) — transport-layer pipe.
    ///   * A MemoryOutputPipe for fd 2 (stderr) — transport-layer pipe.
    ///
    /// `stdout_cap` and `stderr_cap` are accepted but the transport pipes are
    /// uncapped: SPEC.md B-04 requires that overflowing the OutputBuffer limit
    /// is a non-error outcome. A capped WASI pipe would produce a real trap.
    fn setup_wasi_pipes(
        &self,
        stdout_cap: i64,
        stderr_cap: i64,
        preamble_bytes: RString,
        source_bytes: RString,
    ) -> Result<(), MagnusError> {
        let _ = (stdout_cap, stderr_cap);

        // Build the two-frame stdin content. Each frame: 4-byte BE u32 length
        // prefix + payload bytes (SPEC.md ABI Signatures — two-frame protocol).
        let preamble: &[u8] = unsafe { preamble_bytes.as_slice() };
        let source: &[u8] = unsafe { source_bytes.as_slice() };

        let mut stdin_content: Vec<u8> = Vec::with_capacity(4 + preamble.len() + 4 + source.len());
        // Frame 1 — preamble
        stdin_content.extend_from_slice(&(preamble.len() as u32).to_be_bytes());
        stdin_content.extend_from_slice(preamble);
        // Frame 2 — user script
        stdin_content.extend_from_slice(&(source.len() as u32).to_be_bytes());
        stdin_content.extend_from_slice(source);

        let stdin_pipe = MemoryInputPipe::new(stdin_content);
        let stdout_pipe = MemoryOutputPipe::new(usize::MAX);
        let stderr_pipe = MemoryOutputPipe::new(usize::MAX);

        let mut builder = WasiCtxBuilder::new();
        builder.stdin(stdin_pipe);
        builder.stdout(stdout_pipe.clone());
        builder.stderr(stderr_pipe.clone());
        let wasi = builder.build_p1();

        {
            let mut store_ref = self.store.0.borrow_mut();
            let data = store_ref.data_mut();
            data.wasi = Some(wasi);
            data.stdout_pipe = Some(stdout_pipe);
            data.stderr_pipe = Some(stderr_pipe);
        }

        Ok(())
    }

    /// Drain the stdout bytes captured during the most recent run.
    ///
    /// Returns a binary Ruby String containing raw bytes from guest fd 1.
    /// The MemoryOutputPipe clone in HostState retains its contents between
    /// calls — it is replaced on the next `setup_wasi_pipes`.
    fn take_stdout(&self) -> Result<RString, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let store_ref = self.store.0.borrow();
        let bytes = store_ref
            .data()
            .stdout_pipe
            .as_ref()
            .map(|p| p.contents())
            .unwrap_or_default();
        Ok(ruby.str_from_slice(&bytes))
    }

    /// Drain the stderr bytes captured during the most recent run.
    ///
    /// Returns a binary Ruby String containing raw bytes from guest fd 2.
    fn take_stderr(&self) -> Result<RString, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let store_ref = self.store.0.borrow();
        let bytes = store_ref
            .data()
            .stderr_pipe
            .as_ref()
            .map(|p| p.contents())
            .unwrap_or_default();
        Ok(ruby.str_from_slice(&bytes))
    }
}

/// Drive a single `__kobako_rpc_call` invocation end-to-end.
///
/// Steps (SPEC B-12 / B-13):
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
    // Sandbox#run — the invariant SPEC Implementation Standards
    // Architecture pins for the host gem — so `Ruby::get()` is always
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

fn read_memory(caller: &mut Caller<'_, HostState>, ptr: i32, len: i32) -> Option<Vec<u8>> {
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
    // common pre-build state where `data/kobako.wasm` has not yet been
    // produced (e.g. fresh clone before `rake compile`).
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
    instance.define_method(
        "known_export_count",
        method!(Instance::known_export_count, 0),
    )?;
    instance.define_method("alloc", method!(Instance::alloc, 1))?;
    instance.define_method("write_memory", method!(Instance::write_memory, 2))?;
    instance.define_method("read_memory", method!(Instance::read_memory, 2))?;
    instance.define_method("run", method!(Instance::run_call, 0))?;
    instance.define_method("take_outcome", method!(Instance::take_outcome, 0))?;
    instance.define_method("set_registry", method!(Instance::set_registry, 1))?;
    instance.define_method("setup_wasi_pipes", method!(Instance::setup_wasi_pipes, 4))?;
    instance.define_method("take_stdout", method!(Instance::take_stdout, 0))?;
    instance.define_method("take_stderr", method!(Instance::take_stderr, 0))?;

    Ok(())
}
