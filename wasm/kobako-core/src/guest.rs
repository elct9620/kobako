//! The Guest ABI contract as a Rust trait.
//!
//! `Guest` turns the export enumeration pinned by docs/wire-codec.md
//! § ABI Signatures into a compiler-checked type; `export_guest!`
//! emits every `#[no_mangle]` export in the invoking crate, so export
//! signatures cannot drift per-guest and this crate itself defines no
//! `#[no_mangle]` symbol (dependency-rlib exports are unreliable —
//! they get dead-code-GC'd by wasm-ld and break on native ELF).

/// One Guest Binary's invocation surface — the three SPEC entry
/// points behind the wasm exports `export_guest!` emits.
///
/// `eval` and `run` are required: each runs one invocation and writes
/// a single Outcome envelope via `crate::abi::write_outcome` /
/// `crate::abi::write_panic` before returning. `yield_to_block`
/// carries a trapping default — a guest without block support panics,
/// which reaches the host as a wasm trap through its existing trap
/// path (no dedicated "no blocks" wire value needed).
pub trait Guest {
    /// `__kobako_eval` — runs one-shot user source from stdin Frame 2.
    fn eval();

    /// `__kobako_run` — entrypoint dispatch; `env` is the invocation
    /// envelope the host wrote into linear memory.
    fn run(env: &[u8]);

    /// `__kobako_yield_to_block` — host-initiated re-entry into a
    /// guest block (docs/behavior.md B-24); `req` carries the yield
    /// arguments, the return value is the packed `(ptr, len)` of the
    /// YieldResponse buffer.
    fn yield_to_block(_req: &[u8]) -> u64 {
        panic!("no block support")
    }
}

/// Emit every wasm export of the Guest ABI in the invoking crate:
/// the three `Guest` trait forwarders, the `crate::abi` shims
/// (`__kobako_alloc` / `__kobako_take_outcome`), and the WASI reactor
/// `_initialize` no-op (linker bookkeeping, excluded from the kobako
/// export count — see the emitted item's doc).
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
            <$guest as $crate::Guest>::eval()
        }

        #[no_mangle]
        pub extern "C" fn __kobako_run(env_ptr: u32, env_len: u32) {
            // SAFETY: the host wrote the invocation envelope at
            // `[env_ptr, env_ptr + env_len)` in guest linear memory
            // before calling this export (docs/wire-codec.md § ABI
            // Signatures); u8 has alignment 1.
            let env = unsafe {
                ::core::slice::from_raw_parts(env_ptr as usize as *const u8, env_len as usize)
            };
            <$guest as $crate::Guest>::run(env)
        }

        #[no_mangle]
        pub extern "C" fn __kobako_yield_to_block(req_ptr: u32, req_len: u32) -> u64 {
            // SAFETY: as for `__kobako_run` — the host wrote the yield
            // arguments at `[req_ptr, req_ptr + req_len)`.
            let req = unsafe {
                ::core::slice::from_raw_parts(req_ptr as usize as *const u8, req_len as usize)
            };
            <$guest as $crate::Guest>::yield_to_block(req)
        }

        #[no_mangle]
        pub extern "C" fn __kobako_alloc(size: u32) -> u32 {
            $crate::abi::alloc(size)
        }

        #[no_mangle]
        pub extern "C" fn __kobako_take_outcome() -> u64 {
            $crate::abi::take_outcome()
        }

        /// WASI Reactor `_initialize` entry-point.
        ///
        /// rust-lld resolves `_initialize` as the reactor entry symbol
        /// when linking a `cdylib` for `wasm32-wasip1`; without an
        /// export the link fails with "entry symbol not defined". The
        /// no-op is sufficient because a kobako guest boots per
        /// invocation inside `__kobako_eval` / `__kobako_run` — no
        /// static ctors or WASI preopens need to run first. Excluded
        /// from the kobako export count (docs/wire-codec.md § ABI
        /// Signatures counts exactly five guest exports).
        #[cfg(target_arch = "wasm32")]
        #[no_mangle]
        pub extern "C" fn _initialize() {}
    };
}

#[cfg(test)]
mod tests {
    use super::Guest;

    /// A guest that opts out of block support by leaving the
    /// `yield_to_block` default in place.
    struct NoBlockGuest;

    impl Guest for NoBlockGuest {
        fn eval() {}
        fn run(_env: &[u8]) {}
    }

    #[test]
    #[should_panic(expected = "no block support")]
    fn default_yield_to_block_panics() {
        // A yield against a guest without block support must trap (the
        // host surfaces the trap as an error); the default impl's
        // panic is that trap under `panic = "abort"`.
        <NoBlockGuest as Guest>::yield_to_block(&[]);
    }
}
