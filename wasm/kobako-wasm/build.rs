// build.rs — kobako-wasm Stage C wiring of the Build Pipeline.
//
// Purpose
// -------
// On the production target (wasm32-wasip1), pull the pre-built mruby static
// archive into the link graph so the cdylib emitted by `cargo build` ends up
// being a complete Guest Binary: codec + envelope + RPC client + mruby VM.
//
// What this file does NOT do (yet)
// --------------------------------
// * It does not run `bindgen`. The mruby C-API binding (`mrb_define_method`,
//   `mrb_state`, `mrb_value`, `mrb_load_string`, …) referenced by item #16
//   (Sandbox#run wiring) lands together with that item. Adding bindgen here
//   would force this build step to depend on a working libclang on every
//   developer machine, which is heavier than item #11's contract requires.
// * It does not validate the mruby archive. The Rake driver
//   (tasks/wasm.rake :: wasm:build) builds libmruby.a immediately before this
//   build script runs, and the wasm-binary invariant test (item #9, executed
//   against the real artefact in test_wasm_guest_build.rb) catches link-time
//   regressions.
//
// Contract with the Rake driver
// -----------------------------
// The Rake driver exports two environment variables before invoking cargo:
//
//   * `MRUBY_LIB_DIR` — absolute path to the directory containing libmruby.a
//     (i.e. `vendor/mruby/build/wasi/lib`). This script emits a
//     `cargo:rustc-link-search=native=$MRUBY_LIB_DIR` and a
//     `cargo:rustc-link-lib=static=mruby`, but ONLY when targeting wasm32 —
//     on the host target (where the rlib is built for unit tests) the mruby
//     C-API symbols are unresolved by design and we leave linkage alone.
//
//   * `WASI_SDK_PATH` — absolute path to the unpacked wasi-sdk root
//     (i.e. `vendor/wasi-sdk`). Reserved for future bindgen integration; the
//     env var is consumed by tasks/wasm.rake to set up CC/AR/linker, not by
//     this file directly.
//
// Idempotency
// -----------
// Cargo only re-runs this script when its source changes or when one of the
// `cargo:rerun-if-env-changed=` entries below changes. Setting these
// explicitly keeps incremental rebuilds cheap.

use std::env;
use std::path::PathBuf;

fn main() {
    // Always re-run when the build-script-relevant env vars change. Without
    // these, cargo's default behavior is to re-run only when build.rs itself
    // is touched, which would mask MRUBY_LIB_DIR rebinding between
    // invocations.
    println!("cargo:rerun-if-env-changed=MRUBY_LIB_DIR");
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/mrb_exc_helper.c");

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    // Host target builds (used for `cargo test` running the codec / envelope
    // unit tests on the developer's machine) MUST NOT pull mruby into the
    // link graph: there is no host libmruby.a in our vendor tree, and the
    // host-target tests do not exercise the mruby C-bridge anyway. Bail out.
    if target_arch != "wasm32" {
        return;
    }

    // Locate vendor/mruby/include relative to the crate root. CARGO_MANIFEST_DIR
    // is always set by cargo; the crate lives at wasm/kobako-wasm/ so
    // ../../vendor/mruby/include resolves to the project-level vendor tree.
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let mruby_include = manifest_dir
        .join("..")
        .join("..")
        .join("vendor")
        .join("mruby")
        .join("include");

    // Compile the layout-safe C shim that exposes `mrb->exc` via a clean
    // FFI function (`kobako_get_exc`). Delegating field-offset knowledge to
    // mruby's own headers means a minor-version bump that reorders struct
    // fields is caught at compile time rather than silently shifting the
    // target word at runtime.
    //
    // Guard: only compile when WASI_SDK_PATH or CC_wasm32_wasip1 is set
    // (i.e. the full wasm:build pipeline). Bare `cargo check` without the
    // wasi-sdk toolchain would use the host clang which lacks the WASI
    // sysroot — mruby.h includes <stdio.h> and would fail. This mirrors
    // the existing link-wiring guard below. The `kobako_get_exc` symbol is
    // still declared in `mruby::sys` under `#[cfg(target_arch = "wasm32")]`
    // which is sufficient for `cargo check` type-checking without a linked
    // object.
    let wasi_sdk = env::var("WASI_SDK_PATH").ok().filter(|s| !s.is_empty());
    let cc_override = env::var("CC_wasm32_wasip1").ok().filter(|s| !s.is_empty());

    if (wasi_sdk.is_some() || cc_override.is_some()) && mruby_include.exists() {
        // The mruby build process generates some headers (e.g.
        // `mruby/presym/id.h`) into the target-specific build directory.
        // We need both the source headers AND the generated build headers
        // in the include path, mirroring how mruby's own build system
        // sets up include dirs for C files that include <mruby.h>.
        let mut build = cc::Build::new();
        build.file("src/mrb_exc_helper.c").include(&mruby_include);

        // Add the wasi build's generated include dir if we can locate it.
        // MRUBY_LIB_DIR is `vendor/mruby/build/wasi/lib`; the generated
        // headers are one level up at `vendor/mruby/build/wasi/include`.
        if let Ok(lib_dir) = env::var("MRUBY_LIB_DIR") {
            if !lib_dir.is_empty() {
                let build_include = PathBuf::from(&lib_dir).join("..").join("include");
                if build_include.exists() {
                    build.include(&build_include);
                }
            }
        }

        build.compile("kobako_exc_helper");
    }

    // wasm32 path: emit link directives only when the Rake driver has staged
    // libmruby.a. In practice `cargo check --target wasm32-wasip1` may run
    // without `MRUBY_LIB_DIR` set (e.g. from `wasm:check`); in that lane we
    // want a clean compile-only signal, so we silently skip the link wiring.
    // The downstream `cargo build` invocation in `wasm:build` always has the
    // env var set (see tasks/wasm.rake), and `mrb_kobako_send` is currently a
    // pure-Rust stub (rpc_client.rs) so even the `cargo build` step links
    // cleanly without mruby symbols when the env var is absent.
    if let Ok(lib_dir) = env::var("MRUBY_LIB_DIR") {
        if !lib_dir.is_empty() {
            println!("cargo:rustc-link-search=native={}", lib_dir);
            println!("cargo:rustc-link-lib=static=mruby");
        }
    }

    // wasi-sdk setjmp library — required because libmruby.a uses setjmp/longjmp
    // via the new WebAssembly exception handling mechanism (build_config/wasi.rb
    // sets `-mllvm -wasm-use-legacy-eh=false`). This produces calls to
    // `__wasm_setjmp`, `__wasm_longjmp`, and `__wasm_setjmp_test` which live in
    // wasi-sdk's libsetjmp.a (not in Rust's wasm32-wasip1 self-contained libc).
    // Without this library, rust-lld's `--allow-undefined` flag would turn these
    // into wasm imports that the host cannot satisfy.
    if let Ok(sdk_path) = env::var("WASI_SDK_PATH") {
        if !sdk_path.is_empty() {
            let setjmp_dir = format!("{}/share/wasi-sysroot/lib/wasm32-wasi", sdk_path);
            println!("cargo:rustc-link-search=native={}", setjmp_dir);
            println!("cargo:rustc-link-lib=static=setjmp");
        }
    }
}
