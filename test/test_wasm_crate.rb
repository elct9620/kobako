# frozen_string_literal: true

# Intentionally does NOT require "test_helper" — like test_gemspec_manifest.rb,
# this test inspects build-time artifacts (the kobako-wasm Rust crate) and
# does not load the Ruby Kobako module.
require "minitest/autorun"
require "open3"

# E2E test for item #4 (Wire codec — Rust crate skeleton).
#
# Two tiers:
#
#   * Fast tier (always runs): scaffolding asserts + `cargo check` +
#     `cargo test` on the host target. Skips with an informative message
#     if `cargo` is not installed.
#   * Real tier (gated by KOBAKO_E2E_BUILD=1): `cargo build --target
#     wasm32-wasip1`. Skips otherwise. Designed to gain teeth once
#     wasi-sdk is vendored and item #6 supplies real codec bodies.
#
# This test enforces the dependency-free invariant by parsing the crate's
# Cargo.toml and asserting no `[dependencies]` table, in addition to
# compiling the code. SPEC.md "Wire Codec" §Codec Choice forbids importing
# a third-party encoder; this test is the gate that keeps the invariant
# from drifting in code review.
class TestWasmCrate < Minitest::Test
  PROJECT_ROOT = File.expand_path("..", __dir__)
  CRATE_DIR    = File.join(PROJECT_ROOT, "wasm", "kobako-wasm")
  CARGO_TOML   = File.join(CRATE_DIR, "Cargo.toml")
  LIB_RS       = File.join(CRATE_DIR, "src", "lib.rs")
  CODEC_RS     = File.join(CRATE_DIR, "src", "codec", "mod.rs")

  def test_crate_scaffolding_exists
    assert File.directory?(CRATE_DIR), "wasm/kobako-wasm/ must exist"
    assert File.file?(CARGO_TOML),     "wasm/kobako-wasm/Cargo.toml must exist"
    assert File.file?(LIB_RS),         "wasm/kobako-wasm/src/lib.rs must exist"
    assert File.file?(CODEC_RS),       "wasm/kobako-wasm/src/codec/mod.rs must exist"
  end

  def test_cargo_toml_declares_kobako_wasm_package
    contents = File.read(CARGO_TOML)
    assert_match(/^\s*name\s*=\s*"kobako-wasm"\s*$/, contents,
                 "Cargo.toml must declare package name `kobako-wasm` per SPEC.md N-5")
    assert_match(/^\s*edition\s*=\s*"2021"\s*$/, contents,
                 "Cargo.toml must pin Rust edition 2021")
  end

  def test_cargo_toml_declares_cdylib_and_rlib_crate_types
    contents = File.read(CARGO_TOML)
    # Allow any whitespace / order inside the array, but require both members.
    assert_match(/crate-type\s*=\s*\[[^\]]*"cdylib"[^\]]*\]/m, contents,
                 "[lib] crate-type must include `cdylib` to produce kobako.wasm")
    assert_match(/crate-type\s*=\s*\[[^\]]*"rlib"[^\]]*\]/m, contents,
                 "[lib] crate-type must include `rlib` so codec tests run on the host")
  end

  def test_cargo_toml_has_no_runtime_dependencies
    contents = File.read(CARGO_TOML)
    deps_section = extract_section(contents, "dependencies")
    assert_nil deps_section,
               "kobako-wasm must be dependency-free per SPEC.md 'Wire Codec' " \
               "§Codec Choice; found a [dependencies] section in Cargo.toml: " \
               "#{deps_section.inspect}"
  end

  def test_cargo_toml_declares_self_workspace
    contents = File.read(CARGO_TOML)
    assert_match(/^\[workspace\]\s*$/, contents,
                 "wasm/kobako-wasm/Cargo.toml must declare itself a workspace " \
                 "root so the host (ext/) and guest (wasm/) build graphs stay " \
                 "separate (tmp/REFERENCE.md Ch.2 §Root Cargo.toml 與 wasm crate 隔離策略)")
  end

  def test_root_cargo_toml_excludes_wasm
    root_cargo = File.join(PROJECT_ROOT, "Cargo.toml")
    contents = File.read(root_cargo)
    assert_match(/exclude\s*=\s*\[[^\]]*"wasm"[^\]]*\]/m, contents,
                 "root Cargo.toml must exclude wasm/ from the host workspace " \
                 "so wasmtime is not pulled into the wasm32 dep graph")
  end

  def test_codec_module_lists_eleven_wire_types
    contents = File.read(CODEC_RS)
    %w[Nil Bool Int UInt Float Str Bin Array Map Handle ErrEnv].each do |variant|
      assert_match(/^\s*#{variant}\b/, contents,
                   "codec::Value enum must include the #{variant} variant " \
                   "(SPEC.md 'Type Mapping' lists 11 wire types)")
    end
  end

  def test_codec_module_declares_wire_error_variants
    contents = File.read(CODEC_RS)
    %w[Truncated InvalidType Utf8].each do |variant|
      assert_match(/^\s*#{variant}\b/, contents,
                   "WireError enum must include the #{variant} variant")
    end
  end

  def test_cargo_check_succeeds_on_host
    skip_unless_cargo
    out, status = Open3.capture2e(
      "cargo", "check", "--manifest-path", CARGO_TOML
    )
    assert status.success?, "cargo check failed:\n#{out}"
  end

  def test_cargo_test_passes_on_host
    skip_unless_cargo
    out, status = Open3.capture2e(
      "cargo", "test", "--manifest-path", CARGO_TOML
    )
    assert status.success?, "cargo test failed:\n#{out}"
    assert_match(/test result: ok\./, out,
                 "cargo test output must report at least one passing test result")
  end

  def test_real_tier_wasm32_wasip1_build
    skip "set KOBAKO_E2E_BUILD=1 to enable the real wasm32-wasip1 build tier" unless ENV["KOBAKO_E2E_BUILD"] == "1"
    skip_unless_cargo

    out, status = Open3.capture2e(
      "cargo", "build", "--manifest-path", CARGO_TOML,
      "--target", "wasm32-wasip1"
    )
    assert status.success?, "cargo build --target wasm32-wasip1 failed:\n#{out}"
  end

  private

  def skip_unless_cargo
    return if system("which cargo > /dev/null 2>&1")

    skip "cargo not installed; install Rust toolchain to exercise the wasm crate"
  end

  # Returns the body of the named top-level table in a Cargo.toml string,
  # or nil if the table is absent or empty (whitespace-only). Treats an
  # empty table the same as absent — the goal of this test is to assert
  # there are no runtime dependencies, and an empty `[dependencies]` is
  # equivalent to no dependencies.
  def extract_section(toml, name)
    re = /^\[#{Regexp.escape(name)}\]\s*\n(.*?)(?=^\[|\z)/m
    match = toml.match(re)
    return nil unless match

    body = match[1]
    # Strip comments and whitespace; if nothing meaningful remains, treat
    # as empty.
    stripped = body.lines.reject { |l| l.strip.empty? || l.strip.start_with?("#") }.join
    stripped.strip.empty? ? nil : body
  end
end
