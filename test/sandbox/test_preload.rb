# frozen_string_literal: true

require "test_helper"

# Sandbox#preload surface tests. Catalog::Snippets validation
# (E-33 / E-34 / non-String code / non-String binary / no-keyword /
# combining binary: with code:|name:) is pinned at the table tier in
# test/catalog/test_snippets.rb; Sandbox#preload is a thin
# delegation. This file holds only the Sandbox-specific contracts:
# chain-returns-self and post-seal rejection (E-35).
#
# Replay-side behaviour (B-32 Result, E-32, E-36, E-37, E-38) is
# exercised end-to-end in test/e2e/test_preload.rb.
class TestSandboxPreload < Minitest::Test
  FIXTURE_PATH = File.expand_path("../fixtures/minimal_abi_ok.wat", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    skip "minimal_abi_ok.wat fixture missing" unless File.exist?(FIXTURE_PATH)
    @sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
  end

  # Sandbox#preload returns self for chaining — distinct from
  # Catalog::Snippets#register, which returns the registered
  # Symbol name (or nil for binary:). This is the only Sandbox-tier
  # contract over Table#register.
  def test_preload_returns_self_for_chaining
    assert_same @sandbox, @sandbox.preload(code: "X = 1", name: :Helper)
    assert_same @sandbox, @sandbox.preload(binary: "RITE")
  end

  # E-35: post-seal #preload calls raise. The minimal_abi_ok.wat
  # fixture stubs the entry points without __kobako_take_outcome, so
  # #eval raises TrapError — but seal! has already fired by then, so
  # the subsequent #preload must raise. The seal-mechanism observable
  # lives on the Sandbox surface; Services#seal! itself is covered in
  # test/catalog/test_services.rb.
  def test_preload_rejects_calls_after_first_invocation
    @sandbox.preload(code: "X = 1", name: :Early)

    assert_raises(Kobako::TrapError) { @sandbox.eval("nil") }

    err = assert_raises(ArgumentError) { @sandbox.preload(code: "Y = 2", name: :Late) }
    assert_match(/after first Sandbox invocation/, err.message)
  end
end
