//! `TCPSocket` over WASI 0.2's `wasi:sockets`, which the guest imports as a
//! core module, as wasi-libc imports preview1. A Sandbox giving the guest no
//! network answers every socket as unsupported, so the guest raises
//! `NotImplementedError`, as it does with no Mount.

use beni::prelude::*;
use beni::{Error, Mrb, RClass, Value};

use crate::errno;

mod wasi {
    wit_bindgen_rust_macro::generate!({
        path: ["wit/deps/io.wit", "wit/deps/clocks.wit", "wit/deps/sockets.wit"],
        world: "wasi:sockets/imports@0.2.12",
        runtime_path: "crate::socket::rt",
        generate_all,
    });
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

use wasi::wasi::sockets::network::{ErrorCode, IpAddressFamily};
use wasi::wasi::sockets::tcp_create_socket::create_tcp_socket;

pub(crate) fn init(mrb: &Mrb) -> Result<(), Error> {
    let socket = mrb.define_class(c"TCPSocket", mrb.class_object())?;
    socket.define_singleton_method(mrb, c"new", beni::method!(tcp_socket_s_new, -1))?;
    socket.define_singleton_method(mrb, c"open", beni::method!(tcp_socket_s_new, -1))?;
    Ok(())
}

/// `TCPSocket.new(host, port)` makes the socket it connects; a Sandbox with
/// no network makes none.
fn tcp_socket_s_new(mrb: &Mrb, _class: RClass, _args: &[Value]) -> Result<Value, Error> {
    match create_tcp_socket(IpAddressFamily::Ipv4) {
        Err(ErrorCode::NotSupported) => Err(errno::not_implemented(
            mrb,
            "TCPSocket.new is unimplemented: this Sandbox gives the guest no network",
        )),
        Err(code) => Err(errno::not_implemented(
            mrb,
            &format!("TCPSocket.new is unimplemented: the network answered {code:?}"),
        )),
        Ok(_socket) => Err(errno::not_implemented(
            mrb,
            "TCPSocket.new is unimplemented: connecting is not yet supported",
        )),
    }
}
