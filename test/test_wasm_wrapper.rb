# frozen_string_literal: true

require "test_helper"

# Wrapper-layer tests for the sole Ruby-visible wasmtime class,
# +Kobako::Wasm::Instance+. The native ext keeps Engine, Module, and Store as
# internal Rust types — they are not reachable from Ruby (SPEC.md "Code
# Organization": `ext/` "exposes no Wasm engine types to the Host App or
# downstream gems").
#
# Scope is limited to the from_path pipeline and its error-mapping surface —
# real-guest export presence is covered transitively by the E2E journeys
# (test_e2e_journeys.rb), which drive +Sandbox#run+ end-to-end and would fail
# fast if any SPEC Wire ABI export went missing.
class TestWasmWrapper < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/minimal.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Instance)
  end

  def test_default_path_resolves_under_project_data_dir
    expected = File.expand_path("../data/kobako.wasm", __dir__)
    assert_equal expected, Kobako::Wasm.default_path
    assert Kobako::Wasm.default_path.start_with?("/"), "default_path must be absolute"
  end

  # ---------- unpack_outcome_ptr_len (ABI bit-level helper) ----------
  #
  # Pure-Ruby decomposition of the +(ptr << 32) | len+ packed u64 that the
  # Rust ext returns from +__kobako_take_outcome+. Covered by unit tests
  # so the bit layout is locked in without depending on a real wasm build.

  def test_unpack_outcome_ptr_len_splits_high_and_low_u32
    packed = (0x1234 << 32) | 0x5678
    assert_equal [0x1234, 0x5678], Kobako::Wasm.unpack_outcome_ptr_len(packed)
  end

  def test_unpack_outcome_ptr_len_handles_zero_value
    assert_equal [0, 0], Kobako::Wasm.unpack_outcome_ptr_len(0)
  end

  def test_unpack_outcome_ptr_len_handles_max_u64
    assert_equal [0xffff_ffff, 0xffff_ffff],
                 Kobako::Wasm.unpack_outcome_ptr_len((1 << 64) - 1)
  end

  def test_unpack_outcome_ptr_len_masks_high_bits_only_in_high_word
    # len-only payload: ptr=0, len=0xffff_ffff
    assert_equal [0, 0xffff_ffff], Kobako::Wasm.unpack_outcome_ptr_len(0xffff_ffff)
  end

  def test_from_path_raises_module_not_built_for_missing_path
    err = assert_raises(Kobako::Wasm::ModuleNotBuiltError) do
      Kobako::Wasm::Instance.from_path("/nonexistent/kobako.wasm")
    end
    assert_match(/rake wasm:build/, err.message)
  end

  def test_module_not_built_error_is_standard_error
    assert_operator Kobako::Wasm::ModuleNotBuiltError, :<, StandardError
    assert_operator Kobako::Wasm::ModuleNotBuiltError, :<, Kobako::Wasm::Error
  end

  def test_from_path_works_with_fixture_module
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)

    instance = Kobako::Wasm::Instance.from_path(FIXTURE_PATH)
    assert_instance_of Kobako::Wasm::Instance, instance
  end

  def test_from_path_repeated_calls_return_independent_instances
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)

    a = Kobako::Wasm::Instance.from_path(FIXTURE_PATH)
    b = Kobako::Wasm::Instance.from_path(FIXTURE_PATH)
    refute_same a, b, "each call must return a fresh Instance with its own Store"
  end
end
