# frozen_string_literal: true

require "test_helper"

# Host pre-flight coverage for Kobako::Sandbox#run
# (docs/behavior/invocation.md E-24 / E-25 / E-29 / E-30): each
# malformed call raises a standard Ruby exception synchronously, before
# any guest involvement — a minimal ABI fixture stands in for the Guest
# Binary so these cases run without it. Guest-driven #run behavior
# lives in test_run.rb.
class TestSandboxRunPreflight < Minitest::Test
  include GuestGuard

  FIXTURE_PATH = TestPaths.fixture("minimal_abi_ok.wat")

  def setup
    require_fixture!(FIXTURE_PATH)
    @fixture_sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
  end

  # @behavior S-092
  def test_e24_target_must_be_symbol_or_string
    err = assert_raises(TypeError) { @fixture_sandbox.run(42) }
    assert_match(/Symbol or String/, err.message)
  end

  # @behavior S-093
  def test_e25_target_must_match_constant_pattern
    err = assert_raises(ArgumentError) { @fixture_sandbox.run(:lowercase) }
    assert_match(/must match/, err.message)
  end

  # @behavior S-094
  # An entrypoint is looked up as one top-level constant, so a nested
  # name is refused at pre-flight rather than resolving to something.
  def test_e25_target_rejects_double_colon_segmented_name
    err = assert_raises(ArgumentError) { @fixture_sandbox.run("Outer::Inner") }
    assert_match(/must match/, err.message)
  end

  # @behavior S-095
  # Legitimate Handles only surface through error fields, so one the
  # caller constructed can only have been smuggled — refusing it at
  # pre-flight keeps the wire layer from ever seeing one here.
  def test_e29_args_must_not_contain_handle
    handle = Kobako::Handle.restore(1)
    err = assert_raises(ArgumentError) { @fixture_sandbox.run(:Worker, handle) }
    assert_match(/Handle/, err.message)
  end

  # @behavior S-096
  # The keyword position is walked separately from the positional one,
  # so a guard covering only the latter would leave this way in open.
  def test_e29_kwargs_values_must_not_contain_handle
    handle = Kobako::Handle.restore(1)
    err = assert_raises(ArgumentError) { @fixture_sandbox.run(:Worker, env: handle) }
    assert_match(/Handle/, err.message)
  end

  # @behavior S-097
  def test_e30_kwargs_keys_must_be_symbols
    err = assert_raises(ArgumentError) { @fixture_sandbox.run(:Worker, **{ "bad" => 1 }) }
    assert_match(/keyword argument keys must be Symbols/, err.message)
  end
end
