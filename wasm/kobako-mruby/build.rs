// build.rs — mirror beni-sys's `mruby_linked` cfg, the same shape as
// the beni crate's own build script.
//
// beni-sys publishes the linked signal through its `links = "mruby"`
// key in every build — `cargo:linked=1` with a real archive linked,
// `cargo:linked=0` in placeholder mode; cargo surfaces it here as
// DEP_MRUBY_LINKED. The gem's mruby-touching internals are gated on
// the derived cfg so placeholder-mode builds (host targets without a
// discovered `libmruby.a`) compile the public surface without
// referencing unlinked FFI symbols.

use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=DEP_MRUBY_LINKED");
    println!("cargo:rustc-check-cfg=cfg(mruby_linked)");
    if env::var("DEP_MRUBY_LINKED").as_deref() == Ok("1") {
        println!("cargo:rustc-cfg=mruby_linked");
    }
}
