//! The Guest ABI contract as a Rust trait.
//!
//! `Guest` turns the operations a downcall carries — eval, run, yield,
//! compile — into a compiler-checked type;
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
    /// the open Call `frame` names. The host counts every Call the guest
    /// has open, block or not, from `0` at the outermost, so a guest
    /// counts them the same way. `args` carries the yield arguments.
    fn yield_to_block(_frame: u32, _args: &[u8]) -> YieldReply {
        YieldReply::Unsupported(Unsupported::new("this guest supports no block"))
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
/// downcall under the guest interface's name, answered through
/// `crate::abi::handle` and a return area, and the Canonical ABI's
/// `cabi_realloc` every list the host writes lands through.
///
/// The downcall enters through `run_ctors_once`, so static constructors run
/// once whether or not the Guest Binary was baked. No `_initialize` is
/// emitted: a bake would run and drop it.
///
/// The invoking crate must be the final `cdylib` shell: exported symbols
/// are only reliable in the linked root crate, and guests build with
/// `panic = "abort"` so a panic becomes a wasm trap, never an unwind
/// across the `extern "C"` boundary.
#[macro_export]
macro_rules! export_guest {
    ($guest:ty) => {
        #[export_name = $crate::__abi_name!(downcall_export)]
        pub extern "C" fn __kobako_downcall(ptr: u32, len: u32) -> u32 {
            $crate::abi::run_ctors_once();
            // SAFETY: the host wrote the request at `[ptr, ptr + len)` in
            // guest linear memory before calling this export; u8 has
            // alignment 1.
            let request =
                unsafe { ::core::slice::from_raw_parts(ptr as usize as *const u8, len as usize) };
            let reply = $crate::abi::handle::<$guest>(request);
            $crate::abi::return_reply(&reply)
        }

        #[no_mangle]
        pub extern "C" fn cabi_realloc(
            old_ptr: u32,
            old_len: u32,
            align: u32,
            new_len: u32,
        ) -> u32 {
            $crate::abi::realloc(old_ptr, old_len, align, new_len)
        }
    };
}
