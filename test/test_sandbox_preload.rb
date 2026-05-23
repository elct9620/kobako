# frozen_string_literal: true

require "test_helper"

# Sandbox#preload surface tests. Catalog::Snippet::Table validation
# (E-33 / E-34 / non-String code / non-String binary / no-keyword /
# combining binary: with code:|name:) is pinned at the table tier in
# test/catalog/test_snippet_table.rb; Sandbox#preload is a thin
# delegation. This file holds only the Sandbox-specific contracts:
# chain-returns-self and post-seal rejection (E-35).
#
# Replay-side behaviour (B-32 Result, E-32, E-36) is exercised
# end-to-end in test/test_e2e_journeys.rb.
class TestSandboxPreload < Minitest::Test
  FIXTURE_PATH = File.expand_path("fixtures/minimal.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    skip "minimal.wasm fixture missing" unless File.exist?(FIXTURE_PATH)
    @sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
  end

  # Sandbox#preload returns self for chaining — distinct from
  # Catalog::Snippet::Table#register, which returns the registered
  # Symbol name (or nil for binary:). This is the only Sandbox-tier
  # contract over Table#register.
  def test_preload_returns_self_for_chaining
    assert_same @sandbox, @sandbox.preload(code: "X = 1", name: :Helper)
    assert_same @sandbox, @sandbox.preload(binary: "RITE")
  end

  # E-35: post-seal #preload calls raise. The minimal.wasm fixture
  # lacks SPEC ABI exports so #eval trips on __kobako_eval and raises
  # TrapError — but seal! has already fired by then, so the subsequent
  # #preload must raise. The seal-mechanism observable lives on the
  # Sandbox surface; Binding#seal! itself is covered in
  # test/catalog/test_binding_namespace.rb.
  def test_preload_rejects_calls_after_first_invocation
    @sandbox.preload(code: "X = 1", name: :Early)

    assert_raises(Kobako::TrapError) { @sandbox.eval("nil") }

    err = assert_raises(ArgumentError) { @sandbox.preload(code: "Y = 2", name: :Late) }
    assert_match(/after first Sandbox invocation/, err.message)
  end
end
