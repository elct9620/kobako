# frozen_string_literal: true

# Real-tier E2E test for item #11 (Guest Binary build pipeline).
#
# Drives the full Stage A → B → C pipeline by invoking `rake wasm:guest`
# against a real vendored toolchain, then asserts the produced
# `data/kobako.wasm`:
#
#   * exists and is non-empty (>= 100 KiB given mruby is linked in);
#   * exposes exactly the SPEC-pinned ABI surface — 1 host import
#     (`env.__kobako_rpc_call`) and 3 guest exports (`__kobako_run`,
#     `__kobako_alloc`, `__kobako_take_outcome`) — applied to the
#     production artefact rather than the bare cdylib;
#   * is idempotent: a second invocation of `rake wasm:guest` is a no-op
#     and leaves the wasm mtime unchanged.
#
# Gated behind `KOBAKO_E2E_BUILD=1` because the build pulls hundreds of MiB
# of toolchain (wasi-sdk + mruby) on first run and may take minutes.
#
# Skips informatively when the env flag is unset OR when the vendored
# toolchain is not pre-fetched. In CI this lane runs after a `vendor:setup`
# warm-up; on developer machines it remains opt-in.

require "minitest/autorun"
require "fileutils"
require "open3"
require "rbconfig"

# Reuse the wasm-binary parser from the ABI invariant test (item #9). That
# parser is a hand-written ~80-line walker over imports/exports/types/
# functions; pulling it in here keeps the invariant definition single-
# sourced.
require_relative "test_abi_wasm_invariant"

class TestWasmGuestBuild < Minitest::Test
  PROJECT_ROOT = File.expand_path("..", __dir__)
  DATA_WASM    = File.join(PROJECT_ROOT, "data", "kobako.wasm")
  VENDOR_DIR   = File.join(PROJECT_ROOT, "vendor")
  WASI_SDK     = File.join(VENDOR_DIR, "wasi-sdk", "bin", "clang")
  MRUBY_DIR    = File.join(VENDOR_DIR, "mruby", "Rakefile")

  def setup
    skip "set KOBAKO_E2E_BUILD=1 to enable the wasm:guest real-tier build" \
      unless ENV["KOBAKO_E2E_BUILD"] == "1"
    skip_unless_cargo
    return if File.exist?(WASI_SDK) && File.exist?(MRUBY_DIR)

    skip "vendored toolchain missing (run `rake vendor:setup` first to enable this test)"
  end

  def test_wasm_guest_produces_valid_kobako_wasm
    out, status = Open3.capture2e(
      RbConfig.ruby, "-S", "rake", "wasm:guest",
      chdir: PROJECT_ROOT
    )
    assert status.success?, "rake wasm:guest failed:\n#{out}"
    assert File.exist?(DATA_WASM), "expected #{DATA_WASM} after wasm:guest"
    size = File.size(DATA_WASM)
    assert_operator size, :>, 100 * 1024,
                    "data/kobako.wasm should exceed 100 KiB once libmruby.a " \
                    "is linked in; got #{size} bytes"

    assert_kobako_abi_invariant_in(DATA_WASM)
  end

  def test_wasm_guest_is_idempotent
    # First invocation: ensure artefact is current.
    out, status = Open3.capture2e(
      RbConfig.ruby, "-S", "rake", "wasm:guest",
      chdir: PROJECT_ROOT
    )
    assert status.success?, "rake wasm:guest (first) failed:\n#{out}"
    first_mtime = File.mtime(DATA_WASM)

    # Sleep one whole second so a re-build (if it incorrectly happened)
    # would produce a strictly later mtime — most filesystems track mtime
    # at second granularity.
    sleep 1.1

    out2, status2 = Open3.capture2e(
      RbConfig.ruby, "-S", "rake", "wasm:guest",
      chdir: PROJECT_ROOT
    )
    assert status2.success?, "rake wasm:guest (second) failed:\n#{out2}"
    assert_match(/up to date|skipping/i, out2,
                 "second invocation should short-circuit, but output was:\n#{out2}")
    assert_equal first_mtime, File.mtime(DATA_WASM),
                 "data/kobako.wasm mtime must be unchanged on idempotent re-run"
  end

  private

  def skip_unless_cargo
    return if system("which cargo > /dev/null 2>&1")

    skip "cargo not installed; install Rust toolchain to exercise wasm:guest"
  end

  # Reuses TestAbiWasmInvariant's parser to apply the SPEC ABI invariant
  # (1 import + 3 exports) to the production data/kobako.wasm artefact.
  # Item #9 already proves this for the bare cdylib; here we re-prove it
  # against the libmruby.a-linked output, where dead-code stripping and
  # wasi-libc imports could otherwise mask drift.
  def assert_kobako_abi_invariant_in(wasm_path)
    helper = TestAbiWasmInvariant.allocate
    parsed = helper.send(:parse_wasm_sections, File.binread(wasm_path))

    require "kobako/abi" unless defined?(Kobako::ABI)

    kobako_imports = parsed[:imports].select do |i|
      i[:module] == Kobako::ABI::IMPORT_MODULE && i[:name].start_with?("__kobako_")
    end
    assert_equal 1, kobako_imports.size,
                 "production wasm must expose exactly 1 kobako-namespaced " \
                 "import (`__kobako_rpc_call`); saw: #{kobako_imports.inspect}"
    assert_equal Kobako::ABI::IMPORT_NAME, kobako_imports.first[:name]

    kobako_exports = parsed[:exports].select { |e| e[:name].start_with?("__kobako_") }
    exported = kobako_exports.map { |e| e[:name] }.sort
    assert_equal Kobako::ABI::EXPORT_NAMES.sort, exported,
                 "production wasm must export exactly 3 kobako-namespaced " \
                 "functions; saw: #{exported.inspect}"
  end
end
