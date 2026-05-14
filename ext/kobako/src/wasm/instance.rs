//! `Kobako::Wasm::Instance` — the only Ruby-visible wasmtime wrapper.
//!
//! Constructed via [`Instance::from_path`]; the wasmtime [`Engine`] and
//! compiled [`Module`] are owned by the [`super::cache`] singletons and
//! never surface to Ruby. The instance wraps a [`StoreCell`] (interior-
//! mutability around `wasmtime::Store<HostState>`) plus two cached
//! [`TypedFunc`] handles for the SPEC ABI exports used by the host-driven
//! run path.
//!
//! The Ruby surface intentionally exposes intent, not the underlying ABI
//! (SPEC.md "Code Organization"). The two-frame stdin protocol, packed-u64
//! outcome encoding, and `__kobako_alloc` / `__kobako_take_outcome` /
//! `__kobako_run` exports are all wrapped inside [`Instance::run`] and
//! [`Instance::outcome`]; Ruby callers see only `#run(preamble, source)`,
//! `#stdout`, `#stderr`, `#outcome!`, and `#registry=`.
//!
//! WASI stdout/stderr capture (SPEC.md B-04): wasmtime-wasi p1 bindings
//! route guest fd 1 and fd 2 into per-run [`MemoryOutputPipe`] instances
//! rebuilt at the start of every [`Instance::run`]. The Ruby `#stdout` /
//! `#stderr` readers expose the raw captured bytes; the [`crate::wasm`]
//! façade and `Kobako::Sandbox` enforce the per-channel cap on top
//! (transport pipes are uncapped because SPEC.md B-04 requires that
//! overflow is a non-error outcome — a capped WASI pipe would trap).
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
    // Cached TypedFunc handles for the two host-driven ABI exports.
    // Optional because test fixtures (a minimal "ping" module) need not
    // provide them; real kobako.wasm always does, and the run-path methods
    // raise a Ruby `Kobako::Wasm::Error` when an export is missing.
    //
    // `__kobako_alloc` is NOT cached here — only `dispatch.rs` calls it,
    // and it does so through `Caller::get_export` on the wasmtime side.
    run: Option<TypedFunc<(), ()>>,
    take_outcome: Option<TypedFunc<(), u64>>,
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

    /// Install the Ruby-side `Kobako::Registry` into HostState. Bound to
    /// Ruby as `Instance#registry=`. From this point on, every
    /// `__kobako_dispatch` import invocation routes through
    /// `registry.dispatch(req_bytes)`.
    pub(crate) fn set_registry(&self, registry: Value) -> Result<(), MagnusError> {
        let mut store_ref = self.store.0.borrow_mut();
        store_ref.data_mut().registry = Some(Opaque::from(registry));
        Ok(())
    }

    // -----------------------------------------------------------------
    // Run-path methods. Each method is best-effort — it raises a Ruby
    // `Kobako::Wasm::Error` when the corresponding export is missing or
    // fails so the Sandbox layer can map errors to the three-class
    // taxonomy.
    // -----------------------------------------------------------------

    /// Execute one guest run.
    ///
    /// Rebuilds the WASI context with fresh stdin / stdout / stderr pipes
    /// (the two-frame stdin protocol carries +preamble+ then +source+ —
    /// SPEC.md ABI Signatures), then invokes `__kobako_run`.
    pub(crate) fn run(&self, preamble: RString, source: RString) -> Result<(), MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        self.refresh_wasi(preamble, source)?;

        let run = self
            .run
            .as_ref()
            .ok_or_else(|| wasm_err(&ruby, "guest does not export __kobako_run"))?;
        let mut store_ref = self.store.0.borrow_mut();
        run.call(store_ref.as_context_mut(), ())
            .map_err(|e| wasm_err(&ruby, format!("__kobako_run(): {}", e)))
    }

    /// Return the stdout bytes captured during the most recent run.
    ///
    /// Non-destructive snapshot of the MemoryOutputPipe contents — calling
    /// repeatedly returns the same bytes until the next `#run` rebuilds the
    /// pipe. Returns an empty binary String before any run.
    pub(crate) fn stdout(&self) -> Result<RString, MagnusError> {
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

    /// Return the stderr bytes captured during the most recent run.
    /// Same semantics as [`Instance::stdout`].
    pub(crate) fn stderr(&self) -> Result<RString, MagnusError> {
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

    /// Read OUTCOME_BUFFER bytes captured during the most recent run.
    /// Bound to Ruby as `Instance#outcome!`. The bang signals that the
    /// underlying `__kobako_take_outcome` export is guest-side destructive
    /// — the buffer pointer is invalidated after this call, so a second
    /// invocation within the same run is undefined — and that any failure
    /// (missing export, length overflow, OOB read) raises
    /// `Kobako::Wasm::Error`.
    pub(crate) fn outcome(&self) -> Result<RString, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let bytes = self.fetch_outcome_bytes(&ruby)?;
        Ok(ruby.str_from_slice(&bytes))
    }

    // -----------------------------------------------------------------
    // Private helpers.
    // -----------------------------------------------------------------

    /// Rebuild the WASI context with fresh stdin (two-frame: preamble then
    /// source) plus fresh stdout/stderr pipes. Called at the top of every
    /// `#run`.
    fn refresh_wasi(&self, preamble: RString, source: RString) -> Result<(), MagnusError> {
        // SAFETY: `as_slice` borrows are scoped to building the stdin Vec
        // below — no Ruby allocations happen between the borrow and the
        // copy, so the underlying RString cannot move.
        let preamble_bytes: &[u8] = unsafe { preamble.as_slice() };
        let source_bytes: &[u8] = unsafe { source.as_slice() };

        let mut stdin_content: Vec<u8> =
            Vec::with_capacity(4 + preamble_bytes.len() + 4 + source_bytes.len());
        // Frame 1 — preamble
        stdin_content.extend_from_slice(&(preamble_bytes.len() as u32).to_be_bytes());
        stdin_content.extend_from_slice(preamble_bytes);
        // Frame 2 — user script
        stdin_content.extend_from_slice(&(source_bytes.len() as u32).to_be_bytes());
        stdin_content.extend_from_slice(source_bytes);

        let stdin_pipe = MemoryInputPipe::new(stdin_content);
        let stdout_pipe = MemoryOutputPipe::new(usize::MAX);
        let stderr_pipe = MemoryOutputPipe::new(usize::MAX);

        let mut builder = WasiCtxBuilder::new();
        builder.stdin(stdin_pipe);
        builder.stdout(stdout_pipe.clone());
        builder.stderr(stderr_pipe.clone());
        let wasi = builder.build_p1();

        let mut store_ref = self.store.0.borrow_mut();
        let data = store_ref.data_mut();
        data.wasi = Some(wasi);
        data.stdout_pipe = Some(stdout_pipe);
        data.stderr_pipe = Some(stderr_pipe);

        Ok(())
    }

    /// Invoke `__kobako_take_outcome`, decode the packed +(ptr<<32)|len+
    /// u64, and copy the OUTCOME_BUFFER slice out of guest memory. Raises
    /// `Kobako::Wasm::Error` when the export is missing, the +ptr+/+len+
    /// arithmetic overflows, the slice falls outside live memory, or the
    /// `memory` export itself is absent.
    fn fetch_outcome_bytes(&self, ruby: &Ruby) -> Result<Vec<u8>, MagnusError> {
        let take = self
            .take_outcome
            .as_ref()
            .ok_or_else(|| wasm_err(ruby, "guest does not export __kobako_take_outcome"))?;

        let mut store_ref = self.store.0.borrow_mut();
        let packed = take
            .call(store_ref.as_context_mut(), ())
            .map_err(|e| wasm_err(ruby, format!("__kobako_take_outcome(): {}", e)))?;
        let ptr = ((packed >> 32) & 0xffff_ffff) as usize;
        let len = (packed & 0xffff_ffff) as usize;

        let mem: Memory = match self.inner.get_export(store_ref.as_context_mut(), "memory") {
            Some(Extern::Memory(m)) => m,
            _ => return Err(wasm_err(ruby, "guest does not export 'memory'")),
        };
        let data = mem.data(store_ref.as_context_mut());
        let end = ptr
            .checked_add(len)
            .ok_or_else(|| wasm_err(ruby, "outcome: ptr + len overflow"))?;
        if end > data.len() {
            return Err(wasm_err(
                ruby,
                format!(
                    "outcome: range [{}, {}) exceeds memory size {}",
                    ptr,
                    end,
                    data.len()
                ),
            ));
        }
        Ok(data[ptr..end].to_vec())
    }
}

/// Build an `Instance` from an engine, module, and store cell. The store
/// cell is moved in and ends up owned by the returned Instance. Wires
/// the WASI p1 imports plus the `__kobako_dispatch` host import.
fn build_instance(
    engine: &WtEngine,
    module: &WtModule,
    store_cell: StoreCell,
) -> Result<Instance, MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    let mut linker: Linker<HostState> = Linker::new(engine);

    // Wire the wasmtime-wasi preview1 WASI imports. Routes guest fd 1/2 to
    // the MemoryOutputPipes set up before each run via `Instance::run`.
    // The closure extracts `&mut WasiP1Ctx` from HostState; if none is set
    // (e.g. a test module that never invokes WASI functions), the Option
    // unwrap will panic — but `Instance::run` always refreshes the context
    // before invoking any WASI-enabled guest export.
    p1::add_to_linker_sync(&mut linker, |state: &mut HostState| {
        state
            .wasi
            .as_mut()
            .expect("WASI context not initialised — call Instance#run before any WASI use")
    })
    .map_err(|e| wasm_err(&ruby, format!("add WASI p1 to linker: {}", e)))?;

    // `__kobako_dispatch` host import. Signature per SPEC Wire ABI:
    //   (req_ptr: i32, req_len: i32) -> i64
    // Decodes the Request bytes, dispatches via the Ruby-side
    // `Kobako::Registry` (set per-run via `set_registry`), allocates a guest
    // buffer through `__kobako_alloc`, writes the Response bytes there, and
    // returns the packed `(ptr<<32)|len`. The dispatcher returns 0 on any
    // wire-layer fault (including a missing Registry); see `dispatch_rpc`.
    linker
        .func_wrap(
            "env",
            "__kobako_dispatch",
            |mut caller: Caller<'_, HostState>, req_ptr: i32, req_len: i32| -> i64 {
                dispatch_rpc(&mut caller, req_ptr, req_len)
            },
        )
        .map_err(|e| wasm_err(&ruby, format!("define __kobako_dispatch: {}", e)))?;

    let instance = {
        let mut store_ref = store_cell.0.borrow_mut();
        linker
            .instantiate(store_ref.as_context_mut(), module)
            .map_err(|e| wasm_err(&ruby, format!("instantiate: {}", e)))?
    };

    // Best-effort export lookup. Missing exports are not an error here
    // (test fixture is a bare module); the host enforces presence at
    // invocation time by raising a Ruby `Kobako::Wasm::Error` when the
    // cached Option is None. Only the SPEC ABI `() -> ()` shape is
    // accepted for `__kobako_run`.
    let (run, take_outcome) = {
        let mut store_ref = store_cell.0.borrow_mut();
        let mut ctx = store_ref.as_context_mut();
        let run = instance
            .get_typed_func::<(), ()>(&mut ctx, "__kobako_run")
            .ok();
        let take_outcome = instance
            .get_typed_func::<(), u64>(&mut ctx, "__kobako_take_outcome")
            .ok();
        (run, take_outcome)
    };

    Ok(Instance {
        inner: instance,
        store: store_cell,
        run,
        take_outcome,
    })
}
