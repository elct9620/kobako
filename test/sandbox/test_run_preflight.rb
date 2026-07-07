# frozen_string_literal: true

require "test_helper"

# Host pre-flight coverage for Kobako::Sandbox#run
# (docs/behavior/invocation.md E-24 / E-25 / E-29 / E-30): each
# malformed call raises a standard Ruby exception synchronously, before
# any guest involvement — a minimal ABI fixture stands in for the Guest
# Binary so these cases run without it. Guest-driven #run behavior
# lives in test_run.rb.
class TestSandboxRunPreflight < Minitest::Test
  FIXTURE_PATH = File.expand_path("../fixtures/minimal_abi_ok.wat", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    skip "minimal_abi_ok.wat fixture missing" unless File.exist?(FIXTURE_PATH)
    @fixture_sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
  end

  # E-24
  def test_e24_target_must_be_symbol_or_string
    err = assert_raises(TypeError) { @fixture_sandbox.run(42) }
    assert_match(/Symbol or String/, err.message)
  end

  # E-25
  def test_e25_target_must_match_constant_pattern
    err = assert_raises(ArgumentError) { @fixture_sandbox.run(:lowercase) }
    assert_match(/must match/, err.message)
  end

  # E-25: `::`-segmented names fail the pattern check at host pre-flight.
  def test_e25_target_rejects_double_colon_segmented_name
    err = assert_raises(ArgumentError) { @fixture_sandbox.run("Outer::Inner") }
    assert_match(/must match/, err.message)
  end

  # E-29 — host pre-flight rejects a forged Handle arriving in args.
  # Legitimate Handles only surface through error fields; a Handle
  # constructed by the caller can only be smuggled, so the wire layer
  # never sees one in this position.
  def test_e29_args_must_not_contain_handle
    handle = Kobako::Handle.restore(1)
    err = assert_raises(ArgumentError) { @fixture_sandbox.run(:Worker, handle) }
    assert_match(/Handle/, err.message)
  end

  # E-29 — kwargs branch of the same rule. A Handle reaching a kwargs
  # value is rejected with the same message structure as the args
  # branch (both go through Transport::Run#forged_handle_message).
  def test_e29_kwargs_values_must_not_contain_handle
    handle = Kobako::Handle.restore(1)
    err = assert_raises(ArgumentError) { @fixture_sandbox.run(:Worker, env: handle) }
    assert_match(/Handle/, err.message)
  end

  # E-30
  def test_e30_kwargs_keys_must_be_symbols
    err = assert_raises(ArgumentError) { @fixture_sandbox.run(:Worker, **{ "bad" => 1 }) }
    assert_match(/keyword argument keys must be Symbols/, err.message)
  end
end
