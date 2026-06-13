//! kobako-baker — bakes the canonical boot state into a linked kobako
//! Guest Binary.
//!
//! `bake` runs wasmtime-wizer over the module, executing its
//! `wizer.initialize` export (the `MrbGuest::bake_boot` body) against a
//! deterministic linker: the WASI surface wasi-libc's reactor
//! `_initialize` touches is stubbed with constant results (mirroring
//! the host's ambient denial), `env::__kobako_dispatch` traps
//! (boot must never dispatch), and any other import fails instantiation
//! loudly. Identical inputs therefore produce identical baked bytes;
//! kobako's Stage C re-bakes and compares to gate reproducibility
//! (F-10).

use wasmtime::Result;

/// `Caller` against the bake's unit store data — the type every WASI
/// stub closure receives.
type Caller<'a> = wasmtime::Caller<'a, ()>;

/// Run the module's `wizer.initialize` export and snapshot the
/// resulting instance state into a new module, returned as bytes.
pub fn bake(wasm: &[u8]) -> Result<Vec<u8>> {
    // The Guest Binary's mruby unwind is wasi-sdk setjmp/longjmp, which
    // compiles to the Wasm exception-handling proposal — opt the bake
    // engine in explicitly.
    let mut config = wasmtime::Config::new();
    config.wasm_exceptions(true);
    let engine = wasmtime::Engine::new(&config)?;
    let mut store = wasmtime::Store::new(&engine, ());
    let mut wizer = wasmtime_wizer::Wizer::new();
    wizer.init_func("wizer.initialize");
    pollster::block_on(wizer.run(
        &mut store,
        wasm,
        |store: &mut wasmtime::Store<()>, module: &wasmtime::Module| {
            let instance = deterministic_linker(store.engine()).and_then(|mut linker| {
                // Imports outside the deterministic set instantiate as
                // traps: declared-but-uncalled WASI surface stays inert,
                // while a boot that actually calls one aborts the bake.
                linker.define_unknown_imports_as_traps(module)?;
                linker.instantiate(store, module)
            });
            async move { instance }
        },
    ))
}

/// Build the bake's import surface: the deterministic WASI stub set
/// plus the trapping `__kobako_dispatch`.
fn deterministic_linker(engine: &wasmtime::Engine) -> Result<wasmtime::Linker<()>> {
    let mut linker = wasmtime::Linker::new(engine);
    add_deterministic_wasi_stubs(&mut linker)?;
    // Boot must never reach the Transport: a dispatch during the bake
    // is a build bug, surfaced as a trap that aborts the bake.
    linker.func_wrap(
        "env",
        "__kobako_dispatch",
        |_req_ptr: u32, _req_len: u32| -> u64 {
            panic!("__kobako_dispatch called during the canonical boot state bake");
        },
    )?;
    Ok(linker)
}

/// The minimal WASI preview1 surface wasi-libc's reactor ctors reach
/// during boot, each answering a constant: no environment, no args, no
/// preopens, frozen clock, zeroed randomness, unwritable stdio. Any
/// import outside this set (and the dispatch stub) stays undefined, so
/// a boot that grows a new ambient dependency fails the bake instead
/// of absorbing nondeterminism.
fn add_deterministic_wasi_stubs(linker: &mut wasmtime::Linker<()>) -> Result<()> {
    const WASI: &str = "wasi_snapshot_preview1";
    const ERRNO_SUCCESS: i32 = 0;
    const ERRNO_BADF: i32 = 8;

    linker.func_wrap(
        WASI,
        "environ_sizes_get",
        |mut caller: Caller, count_ptr: u32, size_ptr: u32| -> i32 {
            write_u32(&mut caller, count_ptr, 0);
            write_u32(&mut caller, size_ptr, 0);
            ERRNO_SUCCESS
        },
    )?;
    linker.func_wrap(
        WASI,
        "environ_get",
        |_caller: Caller, _environ_ptr: u32, _buf_ptr: u32| -> i32 { ERRNO_SUCCESS },
    )?;
    linker.func_wrap(
        WASI,
        "args_sizes_get",
        |mut caller: Caller, count_ptr: u32, size_ptr: u32| -> i32 {
            write_u32(&mut caller, count_ptr, 0);
            write_u32(&mut caller, size_ptr, 0);
            ERRNO_SUCCESS
        },
    )?;
    linker.func_wrap(
        WASI,
        "args_get",
        |_caller: Caller, _argv_ptr: u32, _buf_ptr: u32| -> i32 { ERRNO_SUCCESS },
    )?;
    linker.func_wrap(
        WASI,
        "fd_prestat_get",
        |_caller: Caller, _fd: u32, _buf_ptr: u32| -> i32 { ERRNO_BADF },
    )?;
    linker.func_wrap(
        WASI,
        "clock_time_get",
        |mut caller: Caller, _id: u32, _precision: u64, time_ptr: u32| -> i32 {
            write_u64(&mut caller, time_ptr, 0);
            ERRNO_SUCCESS
        },
    )?;
    linker.func_wrap(
        WASI,
        "random_get",
        |mut caller: Caller, buf_ptr: u32, len: u32| -> i32 {
            let zeros = vec![0u8; len as usize];
            write_bytes(&mut caller, buf_ptr, &zeros);
            ERRNO_SUCCESS
        },
    )?;
    linker.func_wrap(
        WASI,
        "fd_write",
        |_caller: Caller, _fd: u32, _iovs_ptr: u32, _iovs_len: u32, _nwritten_ptr: u32| -> i32 {
            ERRNO_BADF
        },
    )?;
    linker.func_wrap::<_, ()>(WASI, "proc_exit", |_caller: Caller, code: i32| {
        panic!("proc_exit({code}) called during the canonical boot state bake");
    })?;
    Ok(())
}

fn write_u32(caller: &mut Caller, ptr: u32, value: u32) {
    write_bytes(caller, ptr, &value.to_le_bytes());
}

fn write_u64(caller: &mut Caller, ptr: u32, value: u64) {
    write_bytes(caller, ptr, &value.to_le_bytes());
}

fn write_bytes(caller: &mut Caller, ptr: u32, bytes: &[u8]) {
    let memory = caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .expect("guest exports linear memory");
    memory
        .write(caller, ptr as usize, bytes)
        .expect("in-bounds WASI stub write");
}
