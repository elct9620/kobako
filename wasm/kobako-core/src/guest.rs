//! The Guest ABI contract as a Rust trait.
//!
//! `Guest` turns the operations a downcall carries
//! (docs/wire/abi.md § ABI Signatures) into a compiler-checked type;
//! `export_guest!` emits every `#[no_mangle]` export in the invoking
//! crate, so export signatures cannot drift per-guest and this crate
//! itself defines no `#[no_mangle]` symbol (dependency-rlib exports are
//! unreliable — they get dead-code-GC'd by wasm-ld and break on native
//! ELF).

use kobako_transport::envelope::{CompileReply, Outcome, Preamble, Run, Unsupported, YieldReply};

/// One Guest Binary's invocation surface — the operations a downcall
/// names.
///
/// Each entry answers with a value and never touches the wire:
/// `export_guest!` reads the request and the Preamble and frames each
/// answer, so a guest cannot speak a different envelope than the host
/// reads. `eval` and `run` are required. A guest without block support
/// keeps the `yield_to_block` default, and one that does not compile
/// keeps the `compile` default; both answer as unsupported.
pub trait Guest {
    /// The `eval` operation — runs one-shot user source.
    fn eval(preamble: Preamble, source: Vec<u8>) -> Outcome;

    /// The `run` operation — calls the entrypoint the host's Run names.
    fn run(preamble: Preamble, run: Run) -> Outcome;

    /// The `yield` operation — host-initiated re-entry into the block of
    /// the open Call `frame` counts to, from `0` at the outermost; `args`
    /// carries the yield arguments.
    fn yield_to_block(_frame: u32, _args: &[u8]) -> YieldReply {
        YieldReply::Unsupported(Unsupported::new("this guest serves no block"))
    }

    /// The `compile` operation — compiles the Preamble's source Snippets
    /// without running them, so the host can replay their bytecode
    /// instead. A guest that keeps this default answers as unsupported
    /// and the host keeps sending the source.
    fn compile(_preamble: Preamble) -> CompileReply {
        CompileReply::Unsupported(Unsupported::new("this guest does not compile"))
    }
}

/// Emit every wasm export of the Guest ABI in the invoking crate: the
/// downcall, answered through `crate::abi::serve`, and the
/// pre-message shims (`__kobako_alloc` / `__kobako_abi_version`).
///
/// No `_initialize` is emitted — rustc links a wasm32-wasip1 `cdylib`
/// with `--no-entry`, and wasm-ld runs any static constructors
/// through its own export wrappers. A hand-written no-op would
/// suppress that machinery and silently skip ctors instead.
///
/// The invoking crate must be the final `cdylib` shell: `#[no_mangle]`
/// symbols are only reliable in the linked root crate, and guests
/// build with `panic = "abort"` so a panic becomes a wasm trap, never
/// an unwind across the `extern "C"` boundary.
#[macro_export]
macro_rules! export_guest {
    ($guest:ty) => {
        #[no_mangle]
        pub extern "C" fn __kobako_downcall(ptr: u32, len: u32) -> u64 {
            // SAFETY: the host wrote the request at `[ptr, ptr + len)` in
            // guest linear memory before calling this export
            // (docs/wire/abi.md § ABI Signatures); u8 has alignment 1.
            let request =
                unsafe { ::core::slice::from_raw_parts(ptr as usize as *const u8, len as usize) };
            $crate::abi::answer(&$crate::abi::serve::<$guest>(request))
        }

        #[no_mangle]
        pub extern "C" fn __kobako_alloc(size: u32) -> u32 {
            $crate::abi::alloc(size)
        }

        #[no_mangle]
        pub extern "C" fn __kobako_abi_version() -> u32 {
            $crate::abi::ABI_VERSION
        }
    };
}
