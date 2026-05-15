//! Wire ABI surface — host import + guest exports.
//!
//! This module declares the wasm import/export contract pinned by SPEC.md
//! "ABI Signatures". The contract is:
//!
//! * **Exactly 1 host import**: `__kobako_dispatch` — the RPC bridge guest
//!   uses to dispatch a Service call to the host. Lives in the `env`
//!   wasm namespace (`(import "env" "__kobako_dispatch" ...)`).
//! * **Exactly 3 guest exports**:
//!   - `__kobako_run`             — reactor entry; runs boot script
//!   - `__kobako_alloc(size)`     — bump/malloc allocator for buffers
//!   - `__kobako_take_outcome()`  — returns packed (ptr, len) of OUTCOME_BUFFER
//!
//! The import/export name set is enforced at link time: a guest import
//! the host does not provide traps inside wasmtime, and a missing
//! export fails the `link_func_wrap` lookup on the host side or the
//! `Caller::get_export` lookup inside dispatch. E2E journeys
//! (`test/test_e2e_journeys.rb`) drive a full host↔guest round-trip
//! against the real `data/kobako.wasm`, so any name drift surfaces
//! before any other test runs.
//!
//! ## Packed u64 layout
//!
//! Both `__kobako_dispatch` (host import) and `__kobako_take_outcome`
//! (guest export) return a u64 (i64 at the wasm type level) carrying two
//! u32 values: the high 32 bits are the wasm linear memory ptr, the low 32
//! bits are the byte length.
//!
//! ```text
//!  63        32 31         0
//!  ┌──────────┬────────────┐
//!  │   ptr    │    len     │
//!  └──────────┴────────────┘
//!  high 32 bits  low 32 bits
//! ```
//!
//! Extraction: `ptr = (packed >> 32) as u32; len = packed as u32`.
//! Composition: `(ptr as u64) << 32 | len as u64`.
//! `len == 0` is a wire violation (host walks trap path).

#[cfg(target_arch = "wasm32")]
use crate::cstr;

/// Wasm namespace the host import lives in (`env`, per SPEC.md "ABI
/// Signatures").
pub const IMPORT_MODULE: &str = "env";

/// Sole host-provided import function name.
pub const IMPORT_NAME: &str = "__kobako_dispatch";

/// All three guest-provided export names, in declaration order.
pub const EXPORT_NAMES: [&str; 3] = ["__kobako_run", "__kobako_alloc", "__kobako_take_outcome"];

// ---------------------------------------------------------------------------
// Host import declaration.
// ---------------------------------------------------------------------------
//
// The `wasm_import_module = "env"` attribute pins the import namespace.
// Signature: `(req_ptr: i32, req_len: i32) -> i64` per SPEC ABI Signatures.
// We only declare the import on the wasm32 target — on the host target
// (where rlib codec tests run) there is no host to provide the symbol.
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    /// Host-provided RPC bridge. Guest writes a Request payload at
    /// `[req_ptr, req_ptr + req_len)` and calls this; host returns a packed
    /// u64 holding (response_ptr, response_len) of a buffer the host
    /// allocated via `__kobako_alloc` inside the same call frame.
    pub fn __kobako_dispatch(req_ptr: u32, req_len: u32) -> u64;
}

// ---------------------------------------------------------------------------
// Guest exports.
// ---------------------------------------------------------------------------
//
// Signatures must match the SPEC.md "ABI Signatures" table.

/// WASI Reactor `_initialize` entry-point.
///
/// When compiling as a WASI reactor (`cdylib` targeting `wasm32-wasip1`),
/// the rust-lld linker looks for an `_initialize` export to satisfy the
/// reactor CRT model. Without it the link step fails with:
///
///   rust-lld: error: entry symbol not defined: _initialize
///
/// We export a no-op here because wasi-libc reactor init (`crt1-reactor.o`
/// static ctors) is not required for kobako's boot path — kobako creates
/// and destroys an `mrb_state` inside `__kobako_run` for every invocation;
/// there are no static C++ constructors or WASI preopen operations that
/// need to run before the first call. Approach (a) from the two known
/// fixes — smaller and sufficient for the kobako use case.
///
/// Per SPEC.md ABI Signatures, the "exactly 3 kobako exports" invariant
/// counts `__kobako_run`, `__kobako_alloc`, `__kobako_take_outcome`.
/// `_initialize` is a WASI reactor bookkeeping export and is explicitly
/// excluded from the kobako export count.
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn _initialize() {
    // No-op: wasi-libc reactor static ctors are not needed for kobako's
    // reactor model. See comment above.
}

/// Reactor entry — runs the three-job boot script, writing the outcome
/// envelope to OUTCOME_BUFFER before returning. Signature: `() -> ()`.
///
/// Responsibilities:
///
/// 1. Read stdin Frame 1 (4-byte BE u32 length prefix + msgpack preamble).
///    Decode the preamble array (`[["GroupName", ["MemberA"]], ...]`) and
///    install proxy classes via the mruby C API so user scripts can call
///    `GroupName::MemberA.method(...)`.
///
/// 2. Read stdin Frame 2 (4-byte BE u32 length prefix + UTF-8 user script).
///    Evaluate via `mrb_load_nstring`; capture the last-expression value.
///    On mruby exception: build a Panic envelope (origin = "sandbox") and
///    write it to OUTCOME_BUFFER.
///
/// 3. On success: serialize the last-expression value as a Result envelope
///    and write it to OUTCOME_BUFFER.
///
/// `__kobako_run` never traps or calls `exit` — the host reads the outcome
/// tag from `__kobako_take_outcome()` after this function returns.
#[no_mangle]
pub extern "C" fn __kobako_run() {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::codec::Value;
        use crate::mruby::sys;
        use crate::rpc::envelope::{encode_outcome, Outcome, Panic};
        use std::io::Read;

        // --- helpers ---

        fn read_frame() -> Option<Vec<u8>> {
            let mut len_buf = [0u8; crate::FRAME_LEN_SIZE];
            let mut stdin = std::io::stdin().lock();
            stdin.read_exact(&mut len_buf).ok()?;
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut payload = vec![0u8; len];
            stdin.read_exact(&mut payload).ok()?;
            Some(payload)
        }

        // Decode `[["GroupName", ["MemberA", "MemberB"]], ...]` from the
        // Frame 1 msgpack bytes using the kobako wire codec Decoder.
        fn decode_preamble(bytes: &[u8]) -> Option<Vec<(String, Vec<String>)>> {
            use crate::codec::Decoder;
            let mut dec = Decoder::new(bytes);
            let outer = dec.read_value().ok()?;
            let outer_arr = match outer {
                Value::Array(items) => items,
                _ => return None,
            };
            let mut groups = Vec::with_capacity(outer_arr.len());
            for item in outer_arr {
                let pair = match item {
                    Value::Array(p) if p.len() == 2 => p,
                    _ => return None,
                };
                let group_name = match &pair[0] {
                    Value::Str(s) => s.clone(),
                    _ => return None,
                };
                let members_arr = match &pair[1] {
                    Value::Array(m) => m,
                    _ => return None,
                };
                let mut members = Vec::with_capacity(members_arr.len());
                for m in members_arr {
                    match m {
                        Value::Str(s) => members.push(s.clone()),
                        _ => return None,
                    }
                }
                groups.push((group_name, members));
            }
            Some(groups)
        }

        fn write_panic_outcome(origin: &str, class: &str, message: &str, backtrace: Vec<String>) {
            let panic = Panic {
                origin: origin.to_string(),
                class: class.to_string(),
                message: message.to_string(),
                backtrace,
                details: None,
            };
            if let Ok(bytes) = encode_outcome(&Outcome::Panic(panic)) {
                write_outcome(bytes);
            }
            // If serialization itself fails, OUTCOME_BUFFER stays empty —
            // the host treats len=0 as a wire violation → TrapError path
            // (SPEC.md Error Scenarios).
        }

        fn write_outcome(bytes: Vec<u8>) {
            unsafe {
                OUTCOME_BUFFER = bytes;
            }
        }

        // --- Frame 1: read preamble ---

        let frame1 = match read_frame() {
            Some(b) => b,
            None => {
                write_panic_outcome(
                    "sandbox",
                    "Kobako::BootError",
                    "failed to read preamble frame",
                    Vec::new(),
                );
                return;
            }
        };

        let preamble = match decode_preamble(&frame1) {
            Some(p) => p,
            None => {
                write_panic_outcome(
                    "sandbox",
                    "Kobako::BootError",
                    "failed to decode preamble msgpack",
                    Vec::new(),
                );
                return;
            }
        };

        // --- Frame 2: read user script ---

        let frame2 = match read_frame() {
            Some(b) => b,
            None => {
                write_panic_outcome(
                    "sandbox",
                    "Kobako::BootError",
                    "failed to read script frame",
                    Vec::new(),
                );
                return;
            }
        };

        // --- mruby VM init ---
        //
        // `Mrb::open` wraps `mrb_open` with NULL handling and ties the VM
        // lifetime to a Drop guard — every early-return below releases
        // the state automatically.

        let mrb = match crate::mruby::Mrb::open() {
            Ok(m) => m,
            Err(_) => {
                write_panic_outcome(
                    "sandbox",
                    "Kobako::BootError",
                    "mrb_open returned NULL",
                    Vec::new(),
                );
                return;
            }
        };

        // --- Install Kobako runtime and Frame 1 Service Groups ---
        //
        // `Kobako::install` registers `Kobako`, `Kobako::RPC` (module),
        // `Kobako::RPC::Client`, `Kobako::RPC::Handle`, `Kobako::RPC::WireError`, the error classes and `Kernel#puts` / `p`
        // shims. `install_groups` walks the preamble and installs each
        // Group module + Member subclass. Neither runs Ruby source —
        // every entity is registered through the mruby C API.

        let kobako = crate::kobako::Kobako::install(&mrb);

        use crate::kobako::InstallGroupsError;
        match kobako.install_groups(&preamble) {
            Ok(()) => {}
            Err(InstallGroupsError::NulInGroupName) => {
                write_panic_outcome(
                    "sandbox",
                    "Kobako::BootError",
                    "group name contains NUL byte",
                    Vec::new(),
                );
                return;
            }
            Err(InstallGroupsError::NulInMemberName) => {
                write_panic_outcome(
                    "sandbox",
                    "Kobako::BootError",
                    "member name contains NUL byte",
                    Vec::new(),
                );
                return;
            }
        }

        // --- Frame 2: evaluate user script ---
        //
        // `mrb_load_nstring` internally installs its own MRB_TRY frame inside
        // `mrb_vm_exec`. When a Ruby exception is raised, `mrb_vm_exec` catches
        // it, stores the exception object in `mrb->exc`, and returns normally.
        // `mrb_load_nstring` then detects `mrb->exc` and returns `mrb_nil_value()`.
        //
        // This means `mrb_protect_error` + `run_script` callback does NOT work
        // for catching exceptions from `mrb_load_nstring`: the exception is
        // consumed internally by the VM before it reaches the outer protect frame.
        //
        // Correct pattern: call `mrb_load_nstring` directly, then retrieve the
        // pending exception via `kobako_get_exc` (src/mruby/exc.c). That
        // shim accesses `mrb->exc` through mruby's own headers, so the field
        // offset is always correct for the compiler and mruby version in use.

        // Compile under a context with a filename so the resulting IREP
        // carries `debug_info`; `pack_backtrace` in
        // `vendor/mruby/src/backtrace.c` skips any frame whose IREP has
        // no debug info, which is why `Exception#backtrace` returns an
        // empty array when scripts are loaded via the bare
        // `mrb_load_nstring`. SPEC.md "Panic Envelope" L876 mandates a
        // populated `backtrace` field for Panic envelopes.
        let cxt = unsafe { sys::mrb_ccontext_new(mrb.as_ptr()) };
        if cxt.is_null() {
            write_panic_outcome(
                "sandbox",
                "Kobako::BootError",
                "mrb_ccontext_new returned NULL",
                Vec::new(),
            );
            return;
        }
        unsafe { sys::mrb_ccontext_filename(mrb.as_ptr(), cxt, cstr!("(script)")) };
        let result_val = unsafe {
            sys::mrb_load_nstring_cxt(
                mrb.as_ptr(),
                frame2.as_ptr() as *const core::ffi::c_char,
                frame2.len(),
                cxt,
            )
        };
        unsafe { sys::mrb_ccontext_free(mrb.as_ptr(), cxt) };

        // Retrieve the pending exception (if any) via the layout-safe C shim.
        // `kobako_get_exc` returns `mrb_nil_value()` (w == 0 on wasm32) when
        // no exception is pending, or `mrb_obj_value(mrb->exc)` otherwise.
        // Does NOT clear `mrb->exc` — we call `mrb_check_error` below after
        // consuming the exception object.
        let exc_val = unsafe { sys::kobako_get_exc(mrb.as_ptr()) };
        let has_exception = exc_val.w != 0;

        // --- Outcome serialization ---

        if has_exception {
            // Extract class name from the exception object.
            let class_name = unsafe {
                let cn = exc_val.classname(mrb.as_ptr());
                if cn.is_empty() {
                    "RuntimeError".to_string()
                } else {
                    cn.to_string()
                }
            };

            // Call .message on the exception object to get the error message.
            let message = unsafe {
                let m = exc_val
                    .call(mrb.as_ptr(), cstr!("message"), &[])
                    .to_string(mrb.as_ptr());
                if m.is_empty() {
                    class_name.clone()
                } else {
                    m
                }
            };

            // Collect mruby Exception#backtrace before clearing the
            // pending exception (SPEC.md "Panic Envelope" L876).
            let backtrace = kobako.extract_backtrace(exc_val);

            // Clear the exception from mrb state.
            let _ = unsafe { sys::mrb_check_error(mrb.as_ptr()) };

            // Determine origin: Kobako::ServiceError → "service"; others → "sandbox".
            let origin = if class_name.contains("ServiceError") {
                "service"
            } else {
                "sandbox"
            };

            write_panic_outcome(origin, &class_name, &message, backtrace);
        } else {
            // Success: convert mrb_value to wire Value and encode as Result envelope.
            // We use mrb_inspect to get a string representation for conversion.
            // For the production path we encode the last expression value through
            // the wire codec. Use mrb_str_to_cstr after mrb_inspect for string
            // values; for other types use mrb_inspect + parse.
            //
            // Simplified encoding: nil → Nil, true/false → Bool,
            // integers → Int via mrb_inspect + parse, strings → Str via
            // mrb_str_to_cstr, other → Str via mrb_inspect.
            let wire_value = kobako.mrb_value_to_wire_outcome(result_val);

            let outcome = Outcome::Value(wire_value);
            match encode_outcome(&outcome) {
                Ok(bytes) => write_outcome(bytes),
                Err(_) => write_panic_outcome(
                    "sandbox",
                    "Kobako::RPC::WireError",
                    "result envelope encode failed",
                    Vec::new(),
                ),
            }
        }
        // `mrb` drops here — `mrb_close` runs automatically.
    }
}

/// Static outcome buffer — written once by `__kobako_run` and consumed
/// once by `__kobako_take_outcome`. Protected by the single-threaded
/// wasm execution model: only one `__kobako_run` executes at a time and
/// no concurrency is possible inside a single wasm instance.
#[cfg(target_arch = "wasm32")]
static mut OUTCOME_BUFFER: Vec<u8> = Vec::new();

/// Guest allocator — hands out a `size`-byte buffer in wasm linear memory
/// and returns its ptr (u32). Returns 0 on allocation failure (host treats
/// 0 as a trap signal). Signature: `(size: i32) -> i32`.
///
/// Delegates to `malloc` from wasi-libc. The allocated buffer is intentionally
/// not freed — its lifetime is bounded by the wasm instance lifetime (one
/// `Sandbox#run` invocation). The host writes the RPC response into this
/// buffer inside the `__kobako_dispatch` callback, then the response is
/// consumed synchronously before the RPC call returns, so the buffer does
/// not need to outlive the call frame. Instance drop frees all linear memory
/// (SPEC.md Wire ABI exports).
#[no_mangle]
pub extern "C" fn __kobako_alloc(size: u32) -> u32 {
    #[cfg(target_arch = "wasm32")]
    {
        extern "C" {
            fn malloc(size: usize) -> *mut u8;
        }
        let ptr = unsafe { malloc(size as usize) };
        if ptr.is_null() {
            // malloc failure → return 0, host treats 0 as a trap signal
            // per SPEC.md Wire ABI exports.
            return 0;
        }
        ptr as u32
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = size;
        0
    }
}

/// Outcome reader — host calls this after `__kobako_run` returns to fetch
/// the OUTCOME_BUFFER bytes. Returns packed u64 `(ptr << 32) | len`.
/// `len == 0` is a wire violation (SPEC.md ABI Signatures). Signature: `() -> i64`.
///
/// The buffer is owned by the static `OUTCOME_BUFFER`; the host must consume
/// the bytes before the next `__kobako_run` call (each run resets the buffer).
#[no_mangle]
pub extern "C" fn __kobako_take_outcome() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        let bytes = &raw const OUTCOME_BUFFER;
        let bytes = unsafe { &*bytes };
        if bytes.is_empty() {
            return 0; // Wire violation signal; host treats as TrapError path.
        }
        let ptr = bytes.as_ptr() as u32;
        let len = bytes.len() as u32;
        pack_u64(ptr, len)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0
    }
}

// ---------------------------------------------------------------------------
// Packed u64 helpers.
// ---------------------------------------------------------------------------

/// Pack `(ptr, len)` into a single u64: high 32 bits = ptr, low 32 = len.
#[inline]
pub fn pack_u64(ptr: u32, len: u32) -> u64 {
    ((ptr as u64) << 32) | (len as u64)
}

/// Unpack a u64 produced by `pack_u64` back into `(ptr, len)`.
#[inline]
pub fn unpack_u64(packed: u64) -> (u32, u32) {
    let ptr = (packed >> 32) as u32;
    let len = packed as u32;
    (ptr, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_module_name_is_env() {
        // SPEC pins host import to the `env` namespace. Changing this
        // is a wire-breaking change.
        assert_eq!(IMPORT_MODULE, "env");
    }

    #[test]
    fn import_name_matches_spec() {
        assert_eq!(IMPORT_NAME, "__kobako_dispatch");
    }

    #[test]
    fn export_names_match_spec() {
        assert_eq!(
            EXPORT_NAMES,
            ["__kobako_run", "__kobako_alloc", "__kobako_take_outcome"],
        );
    }

    #[test]
    fn pack_unpack_roundtrip_zero() {
        let packed = pack_u64(0, 0);
        assert_eq!(packed, 0);
        assert_eq!(unpack_u64(packed), (0, 0));
    }

    #[test]
    fn pack_unpack_roundtrip_max() {
        let packed = pack_u64(u32::MAX, u32::MAX);
        assert_eq!(packed, u64::MAX);
        assert_eq!(unpack_u64(packed), (u32::MAX, u32::MAX));
    }

    #[test]
    fn pack_unpack_roundtrip_common() {
        // Representative common cases: small ptr + 1 KiB len, page-sized
        // ptr + small len, midrange both.
        for &(ptr, len) in &[
            (0x1000_u32, 1024_u32),
            (0x0001_0000, 4),
            (0x7fff_ffff, 0xffff),
            (1, u32::MAX),
            (u32::MAX, 1),
        ] {
            let packed = pack_u64(ptr, len);
            assert_eq!(
                unpack_u64(packed),
                (ptr, len),
                "roundtrip failed for ({ptr:#x}, {len:#x})"
            );
        }
    }

    #[test]
    fn pack_layout_is_high_ptr_low_len() {
        // SPEC ABI Signatures pins the bit layout: high 32 = ptr, low 32 = len.
        // Verify with a known-distinct ptr / len pair.
        let packed = pack_u64(0xAABB_CCDD, 0x1122_3344);
        assert_eq!(packed, 0xAABB_CCDD_1122_3344);
        assert_eq!((packed >> 32) as u32, 0xAABB_CCDD);
        assert_eq!(packed as u32, 0x1122_3344);
    }
}
