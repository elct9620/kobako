# frozen_string_literal: true

require "test_helper"

# Boundary tests for Kobako::Sandbox#preload — the setup-time snippet
# registration verb (docs/behavior.md B-32 / E-33 / E-34 / E-35).
#
# Replay-side behavior (B-32 Result, E-32, E-36) is exercised end-to-end
# in test_e2e_journeys.rb once the guest learns to read Frame 3; this
# file pins the host-side validation and sealing semantics only.
class TestSandboxPreload < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/minimal.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Instance)
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)
    @sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
  end

  def test_fresh_sandbox_has_empty_snippet_table
    assert_instance_of Kobako::Snippet::Table, @sandbox.snippets
    assert @sandbox.snippets.empty?
  end

  def test_preload_returns_self_for_chaining
    result = @sandbox.preload(code: "X = 1", name: :Helper)

    assert_same @sandbox, result
  end

  def test_preload_registers_snippet_under_symbol_name
    @sandbox.preload(code: "Y = 2", name: :Worker)

    assert_equal [:Worker], @sandbox.snippets.names
  end

  def test_preload_preserves_insertion_order_across_calls
    @sandbox.preload(code: "A", name: :Alpha)
    @sandbox.preload(code: "B", name: :Beta)
    @sandbox.preload(code: "C", name: :Gamma)

    assert_equal %i[Alpha Beta Gamma], @sandbox.snippets.names
  end

  def test_preload_rejects_non_string_code
    err = assert_raises(ArgumentError) { @sandbox.preload(code: nil, name: :Helper) }
    assert_match(/code must be a String/, err.message)
  end

  # E-34
  def test_preload_rejects_name_not_matching_constant_pattern
    err = assert_raises(ArgumentError) { @sandbox.preload(code: "X", name: :lowercase) }
    assert_match(/snippet name must match/, err.message)
  end

  # E-33
  def test_preload_rejects_duplicate_name
    @sandbox.preload(code: "first body", name: :Worker)
    err = assert_raises(ArgumentError) { @sandbox.preload(code: "second body", name: :Worker) }
    assert_match(/already preloaded/, err.message)
  end

  # E-35: post-seal calls raise. The minimal.wasm fixture lacks SPEC ABI
  # exports so #eval trips on __kobako_eval and raises TrapError — but
  # seal! has already fired by then, so subsequent #preload must raise.
  def test_preload_rejects_calls_after_first_invocation
    @sandbox.preload(code: "X = 1", name: :Early)

    assert_raises(Kobako::TrapError) { @sandbox.eval("nil") }
    assert @sandbox.services.sealed?

    err = assert_raises(ArgumentError) { @sandbox.preload(code: "Y = 2", name: :Late) }
    assert_match(/after first Sandbox invocation/, err.message)

    # Pre-invocation snippet remains accessible.
    assert_equal [:Early], @sandbox.snippets.names
  end
end
