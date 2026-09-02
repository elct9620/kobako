# frozen_string_literal: true

require "test_helper"

# docs/behavior/security.md B-54: Sandbox.new(profile:) requests the
# isolation rung the runtime builds and declares. This class covers the
# request path through the real ext; the floor-check branches (E-49 and
# the fail-closed off-ladder ranking) live with the PROFILES ladder
# owner and are witnessed on SandboxOptions#enforce_floor! in
# test_sandbox_options.rb — the bundled runtime always builds the
# requested rung, so no real runtime reaches them.
class TestSandboxProfile < Minitest::Test
  include GuestGuard

  FIXTURE_PATH = TestPaths.fixture("minimal_abi_ok.wat")

  def setup
    require_fixture!(FIXTURE_PATH)
  end

  # @behavior RT-015
  # The bundled runtime builds whichever rung is requested, so
  # construction succeeds at both and the reader reports the request.
  def test_profile_defaults_to_hermetic_and_constructs_at_every_ladder_rung
    assert_equal :hermetic, Kobako::Sandbox.new(wasm_path: FIXTURE_PATH).profile,
                 "Sandbox.new without profile: must default to the :hermetic rung"
    assert_equal :permissive, Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, profile: :permissive).profile,
                 "profile: :permissive through Sandbox.new must construct and read back the requested rung"
  end

  # @behavior RT-033 RT-034
  # Sandbox.new forwards every non-wasm_path keyword verbatim to
  # SandboxOptions, so both refusals have to surface through the Sandbox
  # entry point unchanged — a forwarding that swallowed either would
  # leave the per-value coverage in test_sandbox_options.rb unreachable.
  def test_option_keywords_forward_to_sandbox_options_rejection
    assert_raises(ArgumentError, "a non-ladder profile through Sandbox.new must be rejected (E-39)") do
      Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, profile: :sealed)
    end
    assert_raises(ArgumentError, "an unknown keyword through Sandbox.new must be rejected") do
      Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, bogus: 1)
    end
  end
end
