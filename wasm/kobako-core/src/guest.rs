//! The Guest ABI contract as a Rust trait.
//!
//! `Guest` turns the export enumeration pinned by docs/wire/abi.md
//! § ABI Signatures into a compiler-checked type; `export_guest!`
//! emits every `#[no_mangle]` export in the invoking crate, so export
//! signatures cannot drift per-guest and this crate itself defines no
//! `#[no_mangle]` symbol (dependency-rlib exports are unreliable —
//! they get dead-code-GC'd by wasm-ld and break on native ELF).

use kobako_transport::envelope::{Frames, Outcome, Run, YieldReply};

/// One Guest Binary's invocation surface — the three entry
/// points behind the wasm exports `export_guest!` emits.
///
/// Each entry answers with a value and never touches the wire:
/// `export_guest!` reads the Frames and the Run and frames each answer,
/// so a guest cannot speak a different envelope than the host reads. `eval` and
/// `run` are required. `yield_to_block` carries a trapping default — a
/// guest without block support panics, which reaches the host as a
/// wasm trap through its existing trap path (no dedicated "no blocks"
/// wire value needed).
pub trait Guest {
    /// `__kobako_eval` — runs one-shot user source.
    fn eval(frames: Frames, source: Vec<u8>) -> Outcome;

    /// `__kobako_run` — calls the entrypoint the host's Run names.
    fn run(frames: Frames, run: Run) -> Outcome;

    /// `__kobako_yield_to_block` — host-initiated re-entry into a
    /// guest block; `req` carries the yield arguments.
    fn yield_to_block(_req: &[u8]) -> YieldReply {
        panic!("no block support")
    }
}

/// Emit every wasm export of the Guest ABI in the invoking crate:
/// the three `Guest` trait forwarders, each framing its answer through
/// `crate::abi`, and the `crate::abi` shims (`__kobako_alloc` /
/// `__kobako_take_outcome` / `__kobako_abi_version`).
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
        pub extern "C" fn __kobako_eval() {
            let outcome = match $crate::frames::read_eval_frames() {
                Ok((frames, source)) => <$guest as $crate::Guest>::eval(frames, source),
                Err(refused) => refused,
            };
            $crate::abi::write_outcome(outcome)
        }

        #[no_mangle]
        pub extern "C" fn __kobako_run(env_ptr: u32, env_len: u32) {
            // SAFETY: the host wrote the Run envelope at
            // `[env_ptr, env_ptr + env_len)` in guest linear memory
            // before calling this export (docs/wire/abi.md § ABI
            // Signatures); u8 has alignment 1.
            let env = unsafe {
                ::core::slice::from_raw_parts(env_ptr as usize as *const u8, env_len as usize)
            };
            let outcome = match $crate::abi::decode_run(env)
                .and_then(|run| $crate::frames::read_frames().map(|frames| (frames, run)))
            {
                Ok((frames, run)) => <$guest as $crate::Guest>::run(frames, run),
                Err(refused) => refused,
            };
            $crate::abi::write_outcome(outcome)
        }

        #[no_mangle]
        pub extern "C" fn __kobako_yield_to_block(req_ptr: u32, req_len: u32) -> u64 {
            // SAFETY: as for `__kobako_run` — the host wrote the yield
            // arguments at `[req_ptr, req_ptr + req_len)`.
            let req = unsafe {
                ::core::slice::from_raw_parts(req_ptr as usize as *const u8, req_len as usize)
            };
            $crate::abi::write_reply(<$guest as $crate::Guest>::yield_to_block(req))
        }

        #[no_mangle]
        pub extern "C" fn __kobako_alloc(size: u32) -> u32 {
            $crate::abi::alloc(size)
        }

        #[no_mangle]
        pub extern "C" fn __kobako_take_outcome() -> u64 {
            $crate::abi::take_outcome()
        }

        #[no_mangle]
        pub extern "C" fn __kobako_abi_version() -> u32 {
            $crate::abi::ABI_VERSION
        }
    };
}

#[cfg(test)]
mod tests {
    use super::Guest;
    use kobako_transport::envelope::{Frames, Outcome, Run};

    /// A guest that opts out of block support by leaving the
    /// `yield_to_block` default in place.
    struct NoBlockGuest;

    impl Guest for NoBlockGuest {
        fn eval(_frames: Frames, _source: Vec<u8>) -> Outcome {
            Outcome::Ok(Vec::new())
        }
        fn run(_frames: Frames, _run: Run) -> Outcome {
            Outcome::Ok(Vec::new())
        }
    }

    // @behavior T-181
    #[test]
    #[should_panic(expected = "no block support")]
    fn default_yield_to_block_panics() {
        // A yield against a guest without block support must trap (the
        // host surfaces the trap as an error); the default impl's
        // panic is that trap under `panic = "abort"`.
        <NoBlockGuest as Guest>::yield_to_block(&[]);
    }
}
