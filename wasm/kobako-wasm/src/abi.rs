//! Wire ABI surface — host import + guest exports.
//!
//! This module declares the wasm import/export contract pinned by SPEC.md
//! "ABI Signatures". The contract is:
//!
//! * **Exactly 1 host import**: `__kobako_rpc_call` — the RPC bridge guest
//!   uses to dispatch a Service call to the host. Lives in the `env`
//!   wasm namespace (`(import "env" "__kobako_rpc_call" ...)`).
//! * **Exactly 3 guest exports**:
//!   - `__kobako_run`             — reactor entry; runs boot script
//!   - `__kobako_alloc(size)`     — bump/malloc allocator for buffers
//!   - `__kobako_take_outcome()`  — returns packed (ptr, len) of OUTCOME_BUFFER
//!
//! This item delivers the **ABI shape** only. Bodies are stubs marked
//! `unimplemented!()`; later items (#10 boot script, #11 allocator, #12 host
//! linker) fill them in. The build-pipeline guard (item #26) inspects the
//! emitted wasm and verifies exactly these names appear.
//!
//! ## Packed u64 layout
//!
//! Both `__kobako_rpc_call` (host import) and `__kobako_take_outcome`
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

/// Wasm namespace the host import lives in (`env`, per SPEC.md "ABI
/// Signatures").
pub const IMPORT_MODULE: &str = "env";

/// Sole host-provided import function name.
pub const IMPORT_NAME: &str = "__kobako_rpc_call";

/// All three guest-provided export names, in declaration order.
pub const EXPORT_NAMES: [&str; 3] = [
    "__kobako_run",
    "__kobako_alloc",
    "__kobako_take_outcome",
];

// ---------------------------------------------------------------------------
// Host import declaration.
// ---------------------------------------------------------------------------
//
// The `wasm_import_module = "env"` attribute pins the import namespace.
// Signature: `(req_ptr: i32, req_len: i32) -> i64` per SPEC ABI Signatures.
// We only declare the import on the wasm32 target — on the host target
// (where rlib codec tests run) there is no host to provide the symbol.
// The import is also gated on the `abi-exports` feature so downstream
// wasm crates that reuse only the codec/envelope modules (e.g.
// `wasm/test-guest`) can declare their own copy of the import without
// duplicate-symbol errors at link time.
#[cfg(all(target_arch = "wasm32", feature = "abi-exports"))]
#[link(wasm_import_module = "env")]
extern "C" {
    /// Host-provided RPC bridge. Guest writes a Request payload at
    /// `[req_ptr, req_ptr + req_len)` and calls this; host returns a packed
    /// u64 holding (response_ptr, response_len) of a buffer the host
    /// allocated via `__kobako_alloc` inside the same call frame.
    pub fn __kobako_rpc_call(req_ptr: u32, req_len: u32) -> u64;
}

// ---------------------------------------------------------------------------
// Guest exports.
// ---------------------------------------------------------------------------
//
// Signatures must match the SPEC table. Bodies are deliberate stubs — item
// #9 delivers the symbol shape so the build-pipeline guard (item #26) can
// run; later items wire real bodies in.

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
/// Per SPEC.md §ABI Signatures, the "exactly 3 kobako exports" invariant
/// counts `__kobako_run`, `__kobako_alloc`, `__kobako_take_outcome`.
/// `_initialize` is a WASI reactor bookkeeping export and is explicitly
/// excluded from the kobako export count.
#[cfg(all(target_arch = "wasm32", feature = "abi-exports"))]
#[no_mangle]
pub extern "C" fn _initialize() {
    // No-op: wasi-libc reactor static ctors are not needed for kobako's
    // reactor model. See comment above.
}

/// Reactor entry — runs the three-job boot script, writing the outcome
/// envelope to OUTCOME_BUFFER before returning. Signature: `() -> ()`.
///
/// Responsibilities (SPEC.md §Boot Script 三職責):
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
#[cfg(feature = "abi-exports")]
#[no_mangle]
pub extern "C" fn __kobako_run() {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::boot::mrb_kobako_init;
        use crate::codec::Value;
        use crate::envelope::{encode_outcome, Outcome, Panic, ResultEnv};
        use crate::mruby_sys as sys;
        use std::io::Read;

        // --- helpers ---

        fn read_frame() -> Option<Vec<u8>> {
            let mut len_buf = [0u8; 4];
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

        fn write_panic_outcome(origin: &str, class: &str, message: &str) {
            let panic = Panic {
                origin: origin.to_string(),
                class: class.to_string(),
                message: message.to_string(),
                backtrace: vec![],
                details: None,
            };
            if let Ok(bytes) = encode_outcome(&Outcome::Panic(panic)) {
                write_outcome(bytes);
            }
            // If serialization itself fails, OUTCOME_BUFFER stays empty —
            // the host treats len=0 as a wire violation → TrapError path
            // (SPEC.md §Error Scenarios).
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
                write_panic_outcome("sandbox", "Kobako::BootError", "failed to read preamble frame");
                return;
            }
        };

        let preamble = match decode_preamble(&frame1) {
            Some(p) => p,
            None => {
                write_panic_outcome("sandbox", "Kobako::BootError", "failed to decode preamble msgpack");
                return;
            }
        };

        // --- Frame 2: read user script ---

        let frame2 = match read_frame() {
            Some(b) => b,
            None => {
                write_panic_outcome("sandbox", "Kobako::BootError", "failed to read script frame");
                return;
            }
        };

        // --- mruby VM init ---

        let mrb = unsafe { sys::mrb_open() };
        if mrb.is_null() {
            write_panic_outcome("sandbox", "Kobako::BootError", "mrb_open returned NULL");
            return;
        }

        // --- Install Kobako module + Kobako::RPC base class ---

        unsafe { mrb_kobako_init(mrb) };

        // --- Ruby preload: define Kernel#puts (missing from core mruby) ---
        //
        // mruby's core provides Kernel#print (routes to wasi-libc fwrite(stdout))
        // via print.c when mruby-io is absent. Kernel#puts is only defined by the
        // mruby-io gem, which cannot be included in the wasm32-wasip1 build because
        // it uses POSIX <pwd.h> etc. Define a minimal puts on top of print here.
        // Any parse/runtime error in the preload aborts with a BootError.
        {
            let preload: &[u8] = br#"
# Kernel#puts: not available in core mruby without mruby-io.
# mruby-io requires POSIX <pwd.h> absent in wasm32-wasip1.
# Implement on top of Kernel#print (always available in core mruby).
module Kernel
  private
  def puts(*args)
    args = [''] if args.empty?
    args.each do |a|
      if a.is_a?(Array)
        puts(*a)
      else
        s = a.to_s
        print s
        print "\n" unless s.length > 0 && s.getbyte(s.length - 1) == 10
      end
    end
    nil
  end
  def p(*args)
    args.each { |a| print a.inspect; print "\n" }
    args.length == 1 ? args[0] : args
  end
end

# Kobako::ServiceError: stub class used by the C bridge to distinguish
# service-origin exceptions from sandbox-origin exceptions. The class
# name is checked on the host side (Sandbox#build_panic_error) to route
# to Kobako::ServiceError vs. Kobako::SandboxError.
module Kobako
  class ServiceError < RuntimeError; end
  class WireError < RuntimeError; end
end
"#;
            unsafe {
                sys::mrb_load_nstring(
                    mrb,
                    preload.as_ptr() as *const core::ffi::c_char,
                    preload.len(),
                )
            };
            // Preload should never fail, but clear any error defensively.
            let preload_err = unsafe { sys::mrb_check_error(mrb) };
            if preload_err != 0 {
                unsafe { sys::mrb_close(mrb) };
                write_panic_outcome(
                    "sandbox",
                    "Kobako::BootError",
                    "mruby puts preload failed",
                );
                return;
            }
        }

        // --- Install Service Group modules + Member subclasses (Frame 1) ---

        for (group_name, members) in &preamble {
            // NUL-terminate for the C API.
            let group_cstr = match std::ffi::CString::new(group_name.as_str()) {
                Ok(s) => s,
                Err(_) => {
                    unsafe { sys::mrb_close(mrb) };
                    write_panic_outcome("sandbox", "Kobako::BootError", "group name contains NUL byte");
                    return;
                }
            };

            let group_mod = unsafe { sys::mrb_define_module(mrb, group_cstr.as_ptr()) };

            // Retrieve Kobako::RPC class pointer to use as the parent for
            // each Member subclass.
            let kobako_mod = unsafe {
                sys::mrb_define_module(mrb, b"Kobako\0".as_ptr() as *const core::ffi::c_char)
            };
            let rpc_class = unsafe {
                sys::mrb_class_get_under(mrb, kobako_mod, b"RPC\0".as_ptr() as *const core::ffi::c_char)
            };

            for member_name in members {
                let member_cstr = match std::ffi::CString::new(member_name.as_str()) {
                    Ok(s) => s,
                    Err(_) => {
                        unsafe { sys::mrb_close(mrb) };
                        write_panic_outcome("sandbox", "Kobako::BootError", "member name contains NUL byte");
                        return;
                    }
                };

                unsafe {
                    sys::mrb_define_class_under(mrb, group_mod, member_cstr.as_ptr(), rpc_class)
                };
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
        // Correct pattern: call `mrb_load_nstring` directly, then inspect
        // `mrb->exc`. We read the exc pointer at the known struct offset (16 bytes
        // on wasm32: jmp+c+root_c+globals = 4×4 bytes, all unconditional fields).
        //
        // mrb_state layout (mruby.h, unconditional top fields on wasm32):
        //   offset  0: struct mrb_jmpbuf *jmp       (4 bytes)
        //   offset  4: struct mrb_context *c        (4 bytes)
        //   offset  8: struct mrb_context *root_c   (4 bytes)
        //   offset 12: struct iv_tbl *globals        (4 bytes)
        //   offset 16: struct RObject *exc           (4 bytes) ← we read this

        let result_val = unsafe {
            sys::mrb_load_nstring(mrb, frame2.as_ptr() as *const core::ffi::c_char, frame2.len())
        };

        // Read mrb->exc at offset 16 (wasm32 layout: 4 pointer fields × 4 bytes).
        // A non-null exc pointer means an exception occurred. The pointer value
        // is also the lower 32 bits of the mrb_value (MRB_WORDBOX_NO_INLINE_FLOAT
        // + MRB_INT32: mrb_value.w == (u32)mrb_ptr(obj_val)).
        let exc_ptr_u32: u32 = unsafe {
            let mrb_as_u32_ptr = mrb as *const u32;
            *mrb_as_u32_ptr.add(4) // offset 16 = index 4 in u32 array
        };
        let has_exception = exc_ptr_u32 != 0;

        // --- Outcome serialization ---

        if has_exception {
            // Build the exception mrb_value from the raw pointer (MRB_WORDBOX layout).
            let exc_val = sys::mrb_value { w: exc_ptr_u32 };

            // Extract class name from the exception object.
            let class_name = unsafe {
                let ptr = sys::mrb_obj_classname(mrb, exc_val);
                if ptr.is_null() {
                    "RuntimeError".to_string()
                } else {
                    core::ffi::CStr::from_ptr(ptr)
                        .to_str()
                        .unwrap_or("RuntimeError")
                        .to_string()
                }
            };

            // Call .message on the exception object to get the error message.
            let msg_val = unsafe {
                sys::mrb_funcall(
                    mrb,
                    exc_val,
                    b"message\0".as_ptr() as *const core::ffi::c_char,
                    0,
                )
            };
            let message = unsafe {
                let ptr = sys::mrb_str_to_cstr(mrb, msg_val);
                if ptr.is_null() {
                    class_name.clone()
                } else {
                    core::ffi::CStr::from_ptr(ptr)
                        .to_str()
                        .unwrap_or(&class_name)
                        .to_string()
                }
            };

            // Clear the exception from mrb state.
            let _ = unsafe { sys::mrb_check_error(mrb) };

            // Determine origin: Kobako::ServiceError → "service"; others → "sandbox".
            let origin = if class_name.contains("ServiceError") {
                "service"
            } else {
                "sandbox"
            };

            unsafe { sys::mrb_close(mrb) };
            write_panic_outcome(origin, &class_name, &message);
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
            let wire_value = unsafe { mrb_value_to_wire(mrb, result_val) };
            unsafe { sys::mrb_close(mrb) };

            let outcome = Outcome::Result(ResultEnv { value: wire_value });
            match encode_outcome(&outcome) {
                Ok(bytes) => write_outcome(bytes),
                Err(_) => write_panic_outcome(
                    "sandbox",
                    "Kobako::WireError",
                    "result envelope encode failed",
                ),
            }
        }
    }
}

/// Convert an `mrb_value` to a kobako wire `Value` for the outcome Result
/// envelope. Only handles the types representable in the kobako wire
/// protocol (SPEC.md §Type Mapping). Non-representable values fall back
/// to a string via `mrb_inspect`.
#[cfg(all(target_arch = "wasm32", feature = "abi-exports"))]
unsafe fn mrb_value_to_wire(mrb: *mut crate::mruby_sys::mrb_state, val: crate::mruby_sys::mrb_value) -> crate::codec::Value {
    use crate::codec::Value;
    use crate::mruby_sys as sys;

    // Use mrb_inspect to get a Ruby-style string representation, then
    // try to decode as a known type from the string.
    // Better: use mrb_type checks via mrb_obj_classname to branch.
    let classname_ptr = sys::mrb_obj_classname(mrb, val);
    let classname = if classname_ptr.is_null() {
        ""
    } else {
        core::ffi::CStr::from_ptr(classname_ptr)
            .to_str()
            .unwrap_or("")
    };

    match classname {
        "NilClass" => Value::Nil,
        "TrueClass" => Value::Bool(true),
        "FalseClass" => Value::Bool(false),
        "Integer" => {
            // Use mrb_inspect → CStr → parse as i64.
            let inspect_val = sys::mrb_funcall(
                mrb, val, b"to_s\0".as_ptr() as *const core::ffi::c_char, 0
            );
            let s_ptr = sys::mrb_str_to_cstr(mrb, inspect_val);
            if s_ptr.is_null() {
                Value::Int(0)
            } else {
                let s = core::ffi::CStr::from_ptr(s_ptr).to_str().unwrap_or("0");
                Value::Int(s.parse::<i64>().unwrap_or(0))
            }
        }
        "Float" => {
            let inspect_val = sys::mrb_funcall(
                mrb, val, b"to_s\0".as_ptr() as *const core::ffi::c_char, 0
            );
            let s_ptr = sys::mrb_str_to_cstr(mrb, inspect_val);
            if s_ptr.is_null() {
                Value::Float(0.0)
            } else {
                let s = core::ffi::CStr::from_ptr(s_ptr).to_str().unwrap_or("0.0");
                Value::Float(s.parse::<f64>().unwrap_or(0.0))
            }
        }
        "String" => {
            let s_ptr = sys::mrb_str_to_cstr(mrb, val);
            if s_ptr.is_null() {
                Value::Str(String::new())
            } else {
                let s = core::ffi::CStr::from_ptr(s_ptr).to_str().unwrap_or("").to_string();
                Value::Str(s)
            }
        }
        "Array" => {
            // Array: fall back to inspect string for the outcome envelope.
            // Full Array wire encoding is a follow-up item.
            let inspect_val = sys::mrb_funcall(
                mrb, val, b"inspect\0".as_ptr() as *const core::ffi::c_char, 0
            );
            let s_ptr = sys::mrb_str_to_cstr(mrb, inspect_val);
            if s_ptr.is_null() {
                Value::Str(String::new())
            } else {
                Value::Str(core::ffi::CStr::from_ptr(s_ptr).to_str().unwrap_or("").to_string())
            }
        }
        "Hash" => {
            // Fall back to inspect string for hashes — no direct pair-iteration
            // without additional C API shims.
            let inspect_val = sys::mrb_funcall(
                mrb, val, b"inspect\0".as_ptr() as *const core::ffi::c_char, 0
            );
            let s_ptr = sys::mrb_str_to_cstr(mrb, inspect_val);
            if s_ptr.is_null() {
                Value::Str("{}".to_string())
            } else {
                Value::Str(core::ffi::CStr::from_ptr(s_ptr).to_str().unwrap_or("{}").to_string())
            }
        }
        _ => {
            // Unknown / non-wire type: use inspect string representation.
            // Callers that need the exact Ruby type should use the RPC path
            // (Service returns a Handle for non-primitive objects).
            let inspect_val = sys::mrb_funcall(
                mrb, val, b"inspect\0".as_ptr() as *const core::ffi::c_char, 0
            );
            let s_ptr = sys::mrb_str_to_cstr(mrb, inspect_val);
            if s_ptr.is_null() {
                Value::Str(String::new())
            } else {
                Value::Str(core::ffi::CStr::from_ptr(s_ptr).to_str().unwrap_or("").to_string())
            }
        }
    }
}

/// Static outcome buffer — written once by `__kobako_run` and consumed
/// once by `__kobako_take_outcome`. Protected by the single-threaded
/// wasm execution model: only one `__kobako_run` executes at a time and
/// no concurrency is possible inside a single wasm instance.
#[cfg(all(target_arch = "wasm32", feature = "abi-exports"))]
static mut OUTCOME_BUFFER: Vec<u8> = Vec::new();

/// Guest allocator — hands out a `size`-byte buffer in wasm linear memory
/// and returns its ptr (u32). Returns 0 on allocation failure (host treats
/// 0 as a trap signal). Signature: `(size: i32) -> i32`.
///
/// Delegates to `malloc` from wasi-libc. The allocated buffer is intentionally
/// not freed — its lifetime is bounded by the wasm instance lifetime (one
/// `Sandbox#run` invocation). The host writes the RPC response into this
/// buffer inside the `__kobako_rpc_call` callback, then the response is
/// consumed synchronously before the RPC call returns, so the buffer does
/// not need to outlive the call frame. Instance drop frees all linear memory
/// (SPEC.md §Wire ABI exports).
#[cfg(feature = "abi-exports")]
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
            // per SPEC.md §Wire ABI exports.
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
/// `len == 0` is a wire violation (SPEC.md §ABI Signatures). Signature: `() -> i64`.
///
/// The buffer is owned by the static `OUTCOME_BUFFER`; the host must consume
/// the bytes before the next `__kobako_run` call (each run resets the buffer).
#[cfg(feature = "abi-exports")]
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
        assert_eq!(IMPORT_NAME, "__kobako_rpc_call");
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
            assert_eq!(unpack_u64(packed), (ptr, len), "roundtrip failed for ({ptr:#x}, {len:#x})");
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
