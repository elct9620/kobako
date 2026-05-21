// build.rs — kobako-mruby-sys link wiring + C shim compilation.
//
// Purpose
// -------
// On the production target (wasm32-wasip1), this build script does
// three things:
//
//   1. Compiles the four layout-safe C shims (`bytecode.c`, `exc.c`,
//      `io.c`, `value.c`) against mruby's own headers so the macro-only
//      mruby APIs (`mrb_obj_value`, `mrb_integer_p`, `RSTRING_PTR`,
//      `mrb_load_irep_buf`) become real `MRB_API` functions Rust can
//      reach through `extern "C"`.
//   2. Emits `cargo:rustc-link-search=native=$MRUBY_LIB_DIR` plus
//      `cargo:rustc-link-lib=static=mruby` so the resulting rlib drags
//      `libmruby.a` into the eventual `kobako-wasm` cdylib's link
//      graph.
//   3. Emits the matching `setjmp` library link directives sourced
//      from `$WASI_SDK_PATH` so mruby's WebAssembly exception handling
//      (`__wasm_setjmp` / `__wasm_longjmp` / `__wasm_setjmp_test`)
//      resolves cleanly against wasi-sdk's `libsetjmp.a`.
//
// On every other target this script is a near no-op: the early-return
// below skips both the cc::Build run and the link directives, because
// host targets do not have `libmruby.a` and the host-target rlib is
// only used for `cargo test` against the codec / outcome / RPC
// envelope unit tests in the consumer crate.
//
// What this file does NOT do
// --------------------------
//   * It does not run `bindgen`. The mruby C-API binding referenced by
//     the consumer crate's call sites lives hand-rolled in `src/lib.rs`.
//     Wiring bindgen into this script would force libclang onto every
//     developer machine; the upcoming bindgen migration is scoped to
//     this crate alone so that cost stays contained.
//   * It does not validate the mruby archive. The Rake driver
//     (`tasks/wasm.rake :: wasm:build`) builds `libmruby.a` immediately
//     before invoking cargo, and the wasm-binary invariant tests in
//     `kobako-wasm`'s E2E suite catch link-time regressions.
//   * It does not precompile `mrblib/*.rb`. That belongs to the
//     `kobako-wasm` crate (whose `build.rs` calls host `mrbc` to
//     produce RITE blobs embedded via `include_bytes!`) — kobako's
//     own Ruby boot code is not a sys-layer concern.
//
// Contract with the Rake driver
// -----------------------------
// The Rake driver exports two environment variables before invoking
// cargo:
//
//   * `MRUBY_LIB_DIR` — absolute path to the directory containing
//     `libmruby.a` (i.e. `vendor/mruby/build/wasi/lib`). Drives the
//     link-search + link-lib directives, and the build-dir include
//     resolution for mruby's generated headers (`mruby/presym/id.h`).
//   * `WASI_SDK_PATH` — absolute path to the unpacked wasi-sdk root
//     (i.e. `vendor/wasi-sdk`). Drives setjmp library resolution and
//     gates the cc::Build run (without the wasi sysroot the C shims'
//     `#include <stdio.h>` would fail to resolve).
//
// Idempotency
// -----------
// Cargo only re-runs this script when its source changes or when one
// of the `cargo:rerun-if-env-changed=` / `cargo:rerun-if-changed=`
// entries below changes.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=MRUBY_LIB_DIR");
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/bytecode.c");
    println!("cargo:rerun-if-changed=src/exc.c");
    println!("cargo:rerun-if-changed=src/io.c");
    println!("cargo:rerun-if-changed=src/value.c");

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    // Host target builds (used for `cargo test` running consumer-side
    // unit tests on the developer's machine) MUST NOT pull mruby into
    // the link graph: there is no host `libmruby.a` in our vendor
    // tree, and host-target tests do not exercise the mruby C-bridge
    // anyway. Bail out.
    if target_arch != "wasm32" {
        return;
    }

    // Locate vendor/mruby/include relative to the crate root.
    // CARGO_MANIFEST_DIR is always set by cargo; this crate lives at
    // `wasm/kobako-mruby-sys/` so `../../vendor/mruby/include`
    // resolves to the project-level vendor tree.
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let mruby_include = manifest_dir
        .join("..")
        .join("..")
        .join("vendor")
        .join("mruby")
        .join("include");

    // Compile the layout-safe C shims that expose mruby's macro-only
    // APIs through clean FFI functions. Delegating field-offset and
    // boxing-bit knowledge to mruby's own headers means a minor-version
    // bump that reorders struct fields is caught at compile time
    // rather than silently shifting the target word at runtime.
    //
    // Guard: only compile when WASI_SDK_PATH or CC_wasm32_wasip1 is
    // set (i.e. the full wasm:build pipeline). Bare `cargo check`
    // without the wasi-sdk toolchain would use the host clang which
    // lacks the WASI sysroot — `mruby.h` includes `<stdio.h>` and
    // would fail. This mirrors the link-wiring guard below.
    let wasi_sdk = env::var("WASI_SDK_PATH").ok().filter(|s| !s.is_empty());
    let cc_override = env::var("CC_wasm32_wasip1").ok().filter(|s| !s.is_empty());

    if (wasi_sdk.is_some() || cc_override.is_some()) && mruby_include.exists() {
        // The mruby build process generates some headers (e.g.
        // `mruby/presym/id.h`) into the target-specific build directory.
        // We need both the source headers AND the generated build
        // headers in the include path, mirroring how mruby's own build
        // system sets up include dirs for C files that include
        // <mruby.h>.
        let mut build = cc::Build::new();
        build
            .file("src/bytecode.c")
            .file("src/exc.c")
            .file("src/io.c")
            .file("src/value.c")
            .include(&mruby_include);

        // Add the wasi build's generated include dir if we can locate
        // it. MRUBY_LIB_DIR is `vendor/mruby/build/wasi/lib`; the
        // generated headers are one level up at
        // `vendor/mruby/build/wasi/include`.
        if let Ok(lib_dir) = env::var("MRUBY_LIB_DIR") {
            if !lib_dir.is_empty() {
                let build_include = PathBuf::from(&lib_dir).join("..").join("include");
                if build_include.exists() {
                    build.include(&build_include);
                }
            }
        }

        build.compile("kobako_exc");
    }

    // wasm32 path: emit link directives only when the Rake driver has
    // staged libmruby.a. In practice `cargo check --target
    // wasm32-wasip1` may run without `MRUBY_LIB_DIR` set (e.g. from
    // `wasm:check`); in that lane we want a clean compile-only signal,
    // so we silently skip the link wiring. The downstream `cargo
    // build` invocation in `wasm:build` always has the env var set
    // (see tasks/wasm.rake).
    if let Ok(lib_dir) = env::var("MRUBY_LIB_DIR") {
        if !lib_dir.is_empty() {
            println!("cargo:rustc-link-search=native={}", lib_dir);
            println!("cargo:rustc-link-lib=static=mruby");
        }
    }

    // wasi-sdk setjmp library — required because libmruby.a uses
    // setjmp/longjmp via the new WebAssembly exception handling
    // mechanism (`build_config/wasi.rb` sets
    // `-mllvm -wasm-use-legacy-eh=false`). This produces calls to
    // `__wasm_setjmp`, `__wasm_longjmp`, and `__wasm_setjmp_test`
    // which live in wasi-sdk's `libsetjmp.a` (not in Rust's
    // wasm32-wasip1 self-contained libc). Without this library,
    // rust-lld's `--allow-undefined` flag would turn these into wasm
    // imports that the host cannot satisfy.
    if let Ok(sdk_path) = env::var("WASI_SDK_PATH") {
        if !sdk_path.is_empty() {
            let setjmp_dir = format!("{}/share/wasi-sysroot/lib/wasm32-wasi", sdk_path);
            println!("cargo:rustc-link-search=native={}", setjmp_dir);
            println!("cargo:rustc-link-lib=static=setjmp");
        }
    }
}
