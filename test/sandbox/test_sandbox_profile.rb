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
  FIXTURE_PATH = File.expand_path("../fixtures/minimal_abi_ok.wat", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    skip "minimal_abi_ok.wat fixture missing" unless File.exist?(FIXTURE_PATH)
  end

  # The bundled runtime builds whichever rung is requested, so
  # construction succeeds at both and the reader reports the request.
  def test_profile_defaults_to_hermetic_and_constructs_at_every_ladder_rung
    assert_equal :hermetic, Kobako::Sandbox.new(wasm_path: FIXTURE_PATH).profile,
                 "Sandbox.new without profile: must default to the :hermetic rung"
    assert_equal :permissive, Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, profile: :permissive).profile,
                 "profile: :permissive through Sandbox.new must construct and read back the requested rung"
  end

  # Sandbox.new forwards every non-wasm_path keyword verbatim to
  # SandboxOptions, so both option validation (E-39, covered per value
  # in test_sandbox_options.rb) and unknown-keyword rejection surface
  # through the Sandbox entry point unchanged.
  def test_option_keywords_forward_to_sandbox_options_rejection
    assert_raises(ArgumentError, "a non-ladder profile through Sandbox.new must be rejected (E-39)") do
      Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, profile: :sealed)
    end
    assert_raises(ArgumentError, "an unknown keyword through Sandbox.new must be rejected") do
      Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, bogus: 1)
    end
  end
end
