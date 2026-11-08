//! The WASI 0.2 interfaces the guest imports as a core module, as wasi-libc
//! imports preview1: one world, so each WIT resource has one Rust type.

wit_bindgen_rust_macro::generate!({
    inline: "
        package kobako:io;

        world guest {
            include wasi:sockets/imports@0.2.12;
            include wasi:http/imports@0.2.12;
        }
    ",
    path: [
        "wit/deps/io.wit",
        "wit/deps/clocks.wit",
        "wit/deps/random.wit",
        "wit/deps/filesystem.wit",
        "wit/deps/sockets.wit",
        "wit/deps/cli.wit",
        "wit/deps/http.wit",
    ],
    world: "kobako:io/guest",
    runtime_path: "crate::wasi::rt",
    generate_all,
});

/// Wait for `pollable` at most `limit`, answering whether it became ready; no
/// limit waits until it does. The host keeps no timer, so the guest bounds a
/// wait by polling a clock beside it, as CRuby's timeouts wait with a limit.
pub(crate) fn wait_within(
    pollable: &wasi::io::poll::Pollable,
    limit: Option<std::time::Duration>,
) -> bool {
    let Some(limit) = limit else {
        pollable.block();
        return true;
    };
    let nanos = u64::try_from(limit.as_nanos()).unwrap_or(u64::MAX);
    let clock = wasi::clocks::monotonic_clock::subscribe_duration(nanos);
    wasi::io::poll::poll(&[pollable, &clock]).contains(&0)
}

/// Write every byte of `bytes`, each write waiting at most `limit` for room;
/// `Ok(false)` once a wait passes its limit.
pub(crate) fn write_within(
    stream: &wasi::io::streams::OutputStream,
    bytes: &[u8],
    limit: Option<std::time::Duration>,
) -> Result<bool, wasi::io::streams::StreamError> {
    let mut rest = bytes;
    while !rest.is_empty() {
        let room = stream.check_write()? as usize;
        if room == 0 {
            if !wait_within(&stream.subscribe(), limit) {
                return Ok(false);
            }
            continue;
        }
        let (now, later) = rest.split_at(room.min(rest.len()));
        stream.write(now)?;
        rest = later;
    }
    stream.flush()?;
    Ok(true)
}

/// What the generated bindings call into, in place of the `wit-bindgen`
/// crate's runtime, whose versioned `cabi_realloc` would join the Guest
/// Binary's exports; the ABI exports `cabi_realloc` itself.
pub(crate) mod rt {
    use std::alloc::{self, Layout};
    use std::ptr::NonNull;

    pub fn maybe_link_cabi_realloc() {}

    /// A buffer the guest lowers a list argument into, freed once the import
    /// returns.
    pub struct Cleanup {
        ptr: NonNull<u8>,
        layout: Layout,
    }

    impl Cleanup {
        /// Room for `layout`, null and nothing to free for a size of zero.
        pub fn new(layout: Layout) -> (*mut u8, Option<Cleanup>) {
            if layout.size() == 0 {
                return (std::ptr::null_mut(), None);
            }
            // SAFETY: `layout` has a non-zero size.
            let ptr = unsafe { alloc::alloc(layout) };
            let Some(ptr) = NonNull::new(ptr) else {
                alloc::handle_alloc_error(layout)
            };
            (ptr.as_ptr(), Some(Cleanup { ptr, layout }))
        }
    }

    impl Drop for Cleanup {
        fn drop(&mut self) {
            // SAFETY: `ptr` was allocated with `layout` in `new`.
            unsafe { alloc::dealloc(self.ptr.as_ptr(), self.layout) }
        }
    }
}
