//! `Kobako::Wasm::Instance` — the only Ruby-visible wasmtime wrapper.
//!
//! Constructed via [`Instance::from_path`]; the wasmtime [`Engine`] and
//! compiled [`Module`] are owned by the [`super::cache`] singletons and
//! never surface to Ruby. The instance wraps a [`StoreCell`] (interior-
//! mutability around `wasmtime::Store<HostState>`) plus three cached
//! [`TypedFunc`] handles for the SPEC ABI exports.
//!
//! WASI stdout/stderr capture (SPEC.md B-04): wasmtime-wasi p1 bindings
//! route guest fd 1 and fd 2 into per-run [`MemoryOutputPipe`] instances.
//! After each run the host drains the pipes via [`Instance::take_stdout`]
//! / [`Instance::take_stderr`] and pushes the raw bytes through Ruby's
//! OutputBuffer (which enforces the cap and `[truncated]` marker).
//! Stdin carries the two-frame length-prefixed protocol: Frame 1
//! (preamble msgpack) followed by Frame 2 (user script UTF-8), each
//! prefixed by a 4-byte big-endian u32 length. Written via
//! [`Instance::setup_wasi_pipes`] before each run (SPEC.md ABI
//! Signatures).
//!
//! [`Engine`]: wasmtime::Engine
//! [`Module`]: wasmtime::Module
//! [`TypedFunc`]: wasmtime::TypedFunc
//! [`MemoryOutputPipe`]: wasmtime_wasi::p2::pipe::MemoryOutputPipe

use std::cell::RefCell;
use std::path::Path;

use magnus::RString;
use magnus::{value::Opaque, Error as MagnusError, Ruby, Value};
use wasmtime::{
    AsContextMut, Caller, Engine as WtEngine, Extern, Instance as WtInstance, Linker, Memory,
    Module as WtModule, Store as WtStore, TypedFunc,
};
use wasmtime_wasi::p1;
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::WasiCtxBuilder;

use super::cache::{cached_module, shared_engine};
use super::dispatch::dispatch_rpc;
use super::host_state::{HostState, StoreCell};
use super::wasm_err;

#[magnus::wrap(class = "Kobako::Wasm::Instance", free_immediately, size)]
pub(crate) struct Instance {
    inner: WtInstance,
    store: StoreCell,
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
    /// Construct an Instance from a wasm file path, using the process-wide
    /// shared Engine and per-path Module cache. The single Ruby-facing
    /// constructor for `Kobako::Wasm::Instance` — Engine and Module are
    /// never visible to Ruby.
    pub(crate) fn from_path(path: String) -> Result<Self, MagnusError> {
        let engine = shared_engine()?;
        let module = cached_module(Path::new(&path))?;
        let store = WtStore::new(engine, HostState::default());
        let store_cell = StoreCell(RefCell::new(store));
        build_instance(engine, &module, store_cell)
    }

    /// Install the Ruby-side `Kobako::Registry` into HostState. Called by
    /// `Kobako::Sandbox` after constructing the Registry; from this point
    /// on, every `__kobako_rpc_call` import invocation routes through
    /// `registry.dispatch(req_bytes)`.
    pub(crate) fn set_registry(&self, registry: Value) -> Result<(), MagnusError> {
        let mut store_ref = self.store.0.borrow_mut();
        store_ref.data_mut().registry = Some(Opaque::from(registry));
        Ok(())
    }

    // -----------------------------------------------------------------
    // Run-path methods. These drive the alloc → write source → run →
    // take_outcome flow from Ruby. Each method is best-effort — it raises
    // a Ruby `Kobako::Wasm::Error` when the corresponding export is
    // missing or fails so the Sandbox layer can map errors to the
    // three-class taxonomy.
    // -----------------------------------------------------------------

    /// Invoke the guest's `__kobako_alloc(size)` export and return the
    /// resulting linear-memory offset. A return of 0 indicates an
    /// allocation failure (caller should treat as a wire violation /
    /// trap).
    pub(crate) fn alloc(&self, size: i32) -> Result<i32, MagnusError> {
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
    pub(crate) fn write_memory(&self, ptr: i32, bytes: RString) -> Result<(), MagnusError> {
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
    pub(crate) fn read_memory(&self, ptr: i32, len: i32) -> Result<RString, MagnusError> {
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
    pub(crate) fn run_call(&self) -> Result<(), MagnusError> {
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
    pub(crate) fn take_outcome(&self) -> Result<u64, MagnusError> {
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
    pub(crate) fn setup_wasi_pipes(
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
    pub(crate) fn take_stdout(&self) -> Result<RString, MagnusError> {
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
    pub(crate) fn take_stderr(&self) -> Result<RString, MagnusError> {
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

/// Build an `Instance` from an engine, module, and store cell. The store
/// cell is moved in and ends up owned by the returned Instance. Wires
/// the WASI p1 imports plus the `__kobako_rpc_call` host import.
fn build_instance(
    engine: &WtEngine,
    module: &WtModule,
    store_cell: StoreCell,
) -> Result<Instance, MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    let mut linker: Linker<HostState> = Linker::new(engine);

    // Wire the wasmtime-wasi preview1 WASI imports. Routes guest fd 1/2 to
    // the MemoryOutputPipes set up before each run via `setup_wasi_pipes`.
    // The closure extracts `&mut WasiP1Ctx` from HostState; if none is set
    // (e.g. a test module that never invokes WASI functions), the Option
    // unwrap will panic — but `setup_wasi_pipes` is always called before any
    // WASI-enabled run.
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
    // `Kobako::Registry` (set per-run via `set_registry`), allocates a guest
    // buffer through `__kobako_alloc`, writes the Response bytes there, and
    // returns the packed `(ptr<<32)|len`. When no Registry is set (test
    // scenarios that never RPC), the legacy recorder behaviour is preserved.
    linker
        .func_wrap(
            "env",
            "__kobako_rpc_call",
            |mut caller: Caller<'_, HostState>, req_ptr: i32, req_len: i32| -> i64 {
                dispatch_rpc(&mut caller, req_ptr, req_len)
            },
        )
        .map_err(|e| wasm_err(&ruby, format!("define __kobako_rpc_call: {}", e)))?;

    let instance = {
        let mut store_ref = store_cell.0.borrow_mut();
        linker
            .instantiate(store_ref.as_context_mut(), module)
            .map_err(|e| wasm_err(&ruby, format!("instantiate: {}", e)))?
    };

    // Best-effort export lookup. Missing exports are not an error here
    // (test fixture is a bare module); the host enforces presence before
    // invocation. Only the SPEC ABI `() -> ()` shape is accepted for
    // `__kobako_run` — the transitional `(ptr, len) -> ()` shape is gone.
    let (run, take_outcome, alloc) = {
        let mut store_ref = store_cell.0.borrow_mut();
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

    Ok(Instance {
        inner: instance,
        store: store_cell,
        run,
        take_outcome,
        alloc,
    })
}
