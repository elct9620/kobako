//! Generates the host side of the WASI 0.2 imports a Guest Binary links as a
//! core module, from the vendored WIT, as wiggle generates preview1's from
//! `witx/`.

#[path = "build/preview2.rs"]
mod preview2;

fn main() {
    let wit = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("wit/deps");
    println!("cargo:rerun-if-changed=build/preview2.rs");
    println!("cargo:rerun-if-changed={}", wit.display());
    let out = std::path::Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"))
        .join("preview2_bindings.rs");
    let source = preview2::generate(&wit).expect("the vendored WIT generates");
    std::fs::write(&out, source).expect("OUT_DIR takes the generated bindings");
}
