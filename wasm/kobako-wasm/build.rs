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
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // Always re-run when the build-script-relevant env vars change. Without
    // these, cargo's default behavior is to re-run only when build.rs itself
    // is touched, which would mask MRUBY_LIB_DIR rebinding between
    // invocations.
    println!("cargo:rerun-if-env-changed=MRUBY_LIB_DIR");
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");
    println!("cargo:rerun-if-env-changed=MRBC_PATH");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/mruby/exc.c");
    println!("cargo:rerun-if-changed=src/mruby/io.c");

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    // Host target builds (used for `cargo test` running the codec / envelope
    // unit tests on the developer's machine) MUST NOT pull mruby into the
    // link graph: there is no host libmruby.a in our vendor tree, and the
    // host-target tests do not exercise the mruby C-bridge anyway. Bail out.
    if target_arch != "wasm32" {
        return;
    }

    // Stage C.5: precompile mrblib/*.rb to RITE-format .mrb blobs that
    // src/kobako/bytecode.rs embeds via `include_bytes!`. The host `mrbc`
    // emerges from the same vendored mruby tree as `libmruby.a` (Stage B
    // builds both); the path is either supplied explicitly by the Rake
    // driver via `MRBC_PATH` or auto-discovered from CARGO_MANIFEST_DIR.
    // When mrbc is unavailable (clean checkout doing `cargo check` before
    // `rake mruby:build`), we drop empty placeholder files so the
    // `include_bytes!` call sites compile; the resulting cdylib would
    // fail at runtime, but at that point `wasm:build` would also have
    // failed for the missing libmruby.a anyway, so the degradation is
    // self-consistent.
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    precompile_mrblib(&manifest_dir, &out_dir);

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
        build
            .file("src/mruby/exc.c")
            .file("src/mruby/io.c")
            .include(&mruby_include);

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

        build.compile("kobako_exc");
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

/// Compile every `mrblib/*.rb` source into a matching `OUT_DIR/<stem>.mrb`
/// RITE blob. `src/kobako/bytecode.rs` `include_bytes!` reads those blobs
/// into compile-time `&[u8]` constants. Resolves `mrbc` from the
/// `MRBC_PATH` env var (set by `tasks/wasm.rake`) first, falling back to
/// the vendored host-target build that Stage B produces at a known
/// project-relative path. When neither resolves, emits empty placeholder
/// blobs so the `include_bytes!` call sites still type-check during a
/// `cargo check` against a clean checkout — the same lane that already
/// tolerates a missing `libmruby.a` for link-time wiring above.
fn precompile_mrblib(manifest_dir: &Path, out_dir: &Path) {
    let mrblib_dir = manifest_dir.join("mrblib");
    let sources = ["io.rb", "kernel.rb"];

    for src in &sources {
        let src_path = mrblib_dir.join(src);
        println!("cargo:rerun-if-changed={}", src_path.display());
    }

    let mrbc = resolve_mrbc(manifest_dir);

    for src in &sources {
        let src_path = mrblib_dir.join(src);
        let stem = src.strip_suffix(".rb").unwrap();
        let dst_path = out_dir.join(format!("{}.mrb", stem));

        if let Some(ref mrbc_bin) = mrbc {
            let status = Command::new(mrbc_bin)
                .arg("-o")
                .arg(&dst_path)
                .arg(&src_path)
                .status()
                .unwrap_or_else(|e| panic!("failed to spawn {}: {}", mrbc_bin.display(), e));
            if !status.success() {
                panic!(
                    "mrbc {} -> {} failed with status {}",
                    src_path.display(),
                    dst_path.display(),
                    status
                );
            }
        } else {
            // No mrbc available — write an empty placeholder so
            // include_bytes! compiles. cargo:warning surfaces the
            // degraded build to the operator.
            fs::write(&dst_path, b"")
                .unwrap_or_else(|e| panic!("write placeholder {}: {}", dst_path.display(), e));
            println!(
                "cargo:warning=mrbc not found; wrote empty {} (run `rake mruby:build`)",
                dst_path.display()
            );
        }
    }
}

/// Locate the host-target `mrbc` binary. Preference order:
///
///   1. `MRBC_PATH` env var (set by `tasks/wasm.rake` cargo_build_env).
///   2. `<crate>/../../vendor/mruby/build/host/bin/mrbc` — the path
///      where `MRuby::Build.new("host")` in `build_config/wasi.rb`
///      drops the binary after `rake mruby:build`.
///
/// Returns `None` when neither resolves to an existing file, so the
/// caller can fall back to writing empty placeholder blobs.
fn resolve_mrbc(manifest_dir: &Path) -> Option<PathBuf> {
    if let Ok(path) = env::var("MRBC_PATH") {
        if !path.is_empty() {
            let candidate = PathBuf::from(path);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    let fallback = manifest_dir
        .join("..")
        .join("..")
        .join("vendor")
        .join("mruby")
        .join("build")
        .join("host")
        .join("bin")
        .join("mrbc");
    if fallback.exists() {
        Some(fallback)
    } else {
        None
    }
}
