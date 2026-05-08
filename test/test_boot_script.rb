# frozen_string_literal: true

# Intentionally does NOT require "test_helper" — like test_wasm_crate.rb
# this test inspects build-time artifacts and does not load the Ruby
# Kobako module.
require "minitest/autorun"

# Boot mechanism alignment test (item #29).
#
# REFERENCE Ch.5 §Boot Script 預載 (lines 944–985) pins the Guest
# Binary boot mechanism to direct mruby C API registrations performed
# from Rust — no `mrb_load_string` of Ruby boot source, no embedded
# `boot.rb` / `include_str!`. This test guards that contract:
#
#   1. `wasm/kobako-wasm/src/boot.rb` does not exist.
#   2. `wasm/kobako-wasm/src/rpc_client.rs` no longer references
#      `BOOT_SCRIPT` or `include_str!("boot.rb")`.
#   3. The Rust-side boot module exists at
#      `wasm/kobako-wasm/src/boot.rs` and exposes the documented
#      `mrb_kobako_init` entry point.
#   4. The mruby_sys FFI module exists at
#      `wasm/kobako-wasm/src/mruby_sys.rs` and declares the
#      registration C API functions REFERENCE Ch.5 names.
#
# The deeper Rust-level assertions (signature checks, NUL-termination
# of C-string constants, mrb_func_t coercion) live in the cargo-test
# harness in `mruby_sys.rs` / `boot.rs` and run via
# `test_wasm_crate.rb`'s `cargo test --lib` invocation. This file's
# job is the high-level structural guard that REFERENCE alignment is
# intact — a missing file or a stray `BOOT_SCRIPT` reference fails
# here loudly and immediately.
class TestBootScript < Minitest::Test
  PROJECT_ROOT = File.expand_path("..", __dir__)
  WASM_SRC_DIR = File.join(PROJECT_ROOT, "wasm", "kobako-wasm", "src")
  BOOT_RB = File.join(WASM_SRC_DIR, "boot.rb")
  RPC_CLIENT_RS = File.join(WASM_SRC_DIR, "rpc_client.rs")
  BOOT_RS = File.join(WASM_SRC_DIR, "boot.rs")
  MRUBY_SYS_RS = File.join(WASM_SRC_DIR, "mruby_sys.rs")

  # ----- Negative guards: previous Ruby boot mechanism is gone. -----

  def test_boot_rb_no_longer_exists
    refute File.exist?(BOOT_RB),
           "wasm/kobako-wasm/src/boot.rb must NOT exist — REFERENCE Ch.5 " \
           "§Boot Script 預載 (line 946) pins the boot mechanism to mruby " \
           "C API registrations performed from Rust, with no Ruby boot " \
           "text loaded into the VM."
  end

  def test_rpc_client_does_not_reference_boot_script_const
    src = File.read(RPC_CLIENT_RS)
    refute_match(/\bBOOT_SCRIPT\b/, src,
                 "rpc_client.rs must not declare or export BOOT_SCRIPT — " \
                 "the Ruby-text boot mechanism is replaced by " \
                 "boot.rs::mrb_kobako_init (REFERENCE Ch.5 line 946).")
    refute_match(/include_str!\s*\(\s*"boot\.rb"\s*\)/, src,
                 "rpc_client.rs must not embed boot.rb via include_str! — " \
                 "see REFERENCE Ch.5 §Boot Script 預載.")
  end

  # ----- Positive guards: new C API boot mechanism is in place. -----

  def test_boot_rs_exists_and_exposes_mrb_kobako_init
    assert File.file?(BOOT_RS),
           "wasm/kobako-wasm/src/boot.rs must exist (item #29) — " \
           "REFERENCE Ch.5 §Boot Script 預載 places the mruby C API " \
           "registrations on the Rust side."
    src = File.read(BOOT_RS)
    assert_match(/pub\s+unsafe\s+fn\s+mrb_kobako_init\b/, src,
                 "boot.rs must expose `pub unsafe fn mrb_kobako_init(...)` — " \
                 "the entry point invoked by __kobako_run during instance " \
                 "setup (item #16 wires this).")
  end

  def test_boot_rs_uses_required_mruby_c_api_functions
    src = File.read(BOOT_RS)
    # REFERENCE Ch.5 lines 948 / 950 / 952 / 959 enumerate the C API
    # functions that perform the three registrations. Each must appear
    # at least once in boot.rs.
    %w[
      mrb_define_module
      mrb_define_class_under
      mrb_define_module_function
      mrb_define_singleton_method
    ].each do |fn|
      assert_includes src, fn,
                      "boot.rs must call #{fn} (REFERENCE Ch.5 §Boot Script " \
                      "預載 lines 944–985)"
    end
  end

  def test_mruby_sys_rs_declares_ffi_block
    assert File.file?(MRUBY_SYS_RS),
           "wasm/kobako-wasm/src/mruby_sys.rs must exist — REFERENCE " \
           "Ch.5 line 979 names this module as the location of the " \
           "extern \"C\" shim wrappers around mruby C API."
    src = File.read(MRUBY_SYS_RS)
    assert_match(/extern\s+"C"\s*\{/, src,
                 "mruby_sys.rs must declare an extern \"C\" FFI block")
    %w[
      mrb_define_module
      mrb_define_class_under
      mrb_define_module_function
      mrb_define_singleton_method
      mrb_class_ptr
      mrb_class_name
    ].each do |fn|
      assert_includes src, fn,
                      "mruby_sys.rs must declare an FFI binding for #{fn} " \
                      "(REFERENCE Ch.5 §Boot Script 預載)"
    end
  end

  def test_lib_rs_wires_boot_and_mruby_sys_modules
    lib_rs = File.join(WASM_SRC_DIR, "lib.rs")
    src = File.read(lib_rs)
    assert_match(/pub\s+mod\s+boot\b/, src,
                 "lib.rs must declare `pub mod boot;`")
    assert_match(/pub\s+mod\s+mruby_sys\b/, src,
                 "lib.rs must declare `pub mod mruby_sys;`")
    assert_match(/pub\s+use\s+boot::mrb_kobako_init\b/, src,
                 "lib.rs must re-export `boot::mrb_kobako_init` so the " \
                 "host-side ext (or item #16's __kobako_run wiring) can " \
                 "reach it without hand-typing the path.")
  end

  # ----- Real tier (gated KOBAKO_E2E_BUILD=1) -----

  def test_real_tier_wasm_crate_still_compiles
    unless ENV["KOBAKO_E2E_BUILD"] == "1"
      skip "needs KOBAKO_E2E_BUILD=1 to build the wasm crate against " \
           "wasm32-wasip1 with libmruby.a in the link graph"
    end

    skip "wasm32-wasip1 build of the new boot mechanism is exercised " \
         "by test_wasm_guest_build.rb / test_wasm_guest_pipeline.rb when " \
         "KOBAKO_E2E_BUILD=1; this slot is reserved for a focused " \
         "boot-mechanism-only check once item #16 wires the bodies."
  end
end
