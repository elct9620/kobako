//! test-guest — minimal wasm fixture for host-side Sandbox#run E2E tests.
//!
//! Does NOT embed mruby; the source bytes are interpreted as a decimal
//! integer (or the literal string `panic` to exercise the failure branch).
//! This fixture exists so host-side flow can be tested without standing up
//! the full mruby+wasi-sdk toolchain.

#![allow(unsafe_code)]

use core::cell::RefCell;
use kobako_wasm::{encode_outcome, Outcome, Panic, ResultEnv, Value};

// Imported so the wasm import table contains `env::__kobako_rpc_call`
// (an exact-shape match of the production guest). Never invoked here, but
// the linker would dead-code-strip an unused extern, so we touch it from
// a never-taken branch in `__kobako_run`.
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    fn __kobako_rpc_call(req_ptr: u32, req_len: u32) -> u64;
}

#[cfg(target_arch = "wasm32")]
static KEEP_ALIVE: bool = false;

// ---------------------------------------------------------------------------
// Bump allocator backing __kobako_alloc.
// ---------------------------------------------------------------------------

const ARENA_SIZE: usize = 64 * 1024;

thread_local! {
    static ARENA: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static ARENA_OFFSET: RefCell<usize> = const { RefCell::new(0) };
    static OUTCOME_BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn ensure_arena() {
    ARENA.with(|a| {
        let mut a = a.borrow_mut();
        if a.is_empty() {
            a.resize(ARENA_SIZE, 0);
        }
    });
}

#[no_mangle]
pub extern "C" fn __kobako_alloc(size: u32) -> u32 {
    ensure_arena();
    let size = size as usize;
    let mut ptr = 0_u32;
    ARENA_OFFSET.with(|o| {
        let mut off = o.borrow_mut();
        if *off + size > ARENA_SIZE {
            ptr = 0;
            return;
        }
        ARENA.with(|a| {
            let a = a.borrow();
            // Compute the linear-memory address by reading the slice's
            // start. wasm32 lets us cast `&u8` to a u32 address.
            let base = a.as_ptr() as u32;
            ptr = base + (*off as u32);
        });
        *off += size;
    });
    ptr
}

// ---------------------------------------------------------------------------
// __kobako_run — read source from the passed pointer/length, build outcome.
// ---------------------------------------------------------------------------

/// Deliberately deviates from SPEC's `() -> ()` shape. For #16, the host
/// passes the source bytes via the alloc/write/run path. The full WASI-
/// stdin frame mechanism is a later item.
#[no_mangle]
pub extern "C" fn __kobako_run(ptr: u32, len: u32) {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        // Volatile read: the optimizer cannot fold the branch away, but
        // KEEP_ALIVE is always false so the call never executes. This is
        // here so the import table contains __kobako_rpc_call for
        // host-side linker verification.
        if core::ptr::read_volatile(&KEEP_ALIVE) {
            let _ = __kobako_rpc_call(0, 0);
        }
    }

    // Read source bytes from linear memory. On wasm32 the host wrote
    // `len` bytes starting at `ptr`; we re-borrow them as a slice.
    let source: &[u8] = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };

    let outcome = build_outcome(source);
    let bytes = encode_outcome(&outcome).expect("encode outcome");

    OUTCOME_BUFFER.with(|b| {
        let mut buf = b.borrow_mut();
        *buf = bytes;
    });
}

fn build_outcome(source: &[u8]) -> Outcome {
    let text = match core::str::from_utf8(source) {
        Ok(s) => s.trim(),
        Err(_) => "",
    };

    if text == "panic" {
        return Outcome::Panic(Panic {
            origin: "sandbox".into(),
            class: "RuntimeError".into(),
            message: "boom".into(),
            backtrace: vec!["test-guest:1".into()],
            details: None,
        });
    }

    // Default: parse as i64 and wrap in a Result envelope.
    match text.parse::<i64>() {
        Ok(n) => Outcome::Result(ResultEnv {
            value: Value::Int(n),
        }),
        Err(_) => Outcome::Result(ResultEnv {
            value: Value::Str(text.to_string()),
        }),
    }
}

// ---------------------------------------------------------------------------
// __kobako_take_outcome — returns packed (ptr, len) of OUTCOME_BUFFER.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn __kobako_take_outcome() -> u64 {
    let mut packed = 0_u64;
    OUTCOME_BUFFER.with(|b| {
        let buf = b.borrow();
        let ptr = buf.as_ptr() as u32;
        let len = buf.len() as u32;
        packed = ((ptr as u64) << 32) | (len as u64);
    });
    packed
}
