# frozen_string_literal: true

require "test_helper"

# Sandbox.new(profile:) requests the isolation profile the runtime builds
# and declares. This class covers the request path through the real ext;
# the floor-check branches (a runtime below the requested profile and the
# fail-closed ranking of an unknown one) live with the PROFILES order's
# owner and are witnessed on SandboxOptions#enforce_floor in
# test_sandbox_options.rb — the bundled runtime always builds the
# requested profile, so no real runtime reaches them.
class TestSandboxProfile < Minitest::Test
  include GuestGuard

  FIXTURE_PATH = TestPaths.fixture("minimal_abi_ok.wat")

  def setup
    require_fixture(FIXTURE_PATH)
  end

  # @behavior RT-015
  # The bundled runtime builds whichever profile is requested, so
  # construction succeeds at both and the reader reports the request.
  def test_profile_defaults_to_hermetic_and_constructs_at_every_profile
    assert_equal :hermetic, Kobako::Sandbox.new(wasm_path: FIXTURE_PATH).profile,
                 "Sandbox.new without profile: must default to the :hermetic profile"
    assert_equal :permissive, Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, profile: :permissive).profile,
                 "profile: :permissive through Sandbox.new must construct and read back the requested profile"
  end

  # @behavior RT-033 RT-034
  # Sandbox.new forwards every non-wasm_path keyword verbatim to
  # SandboxOptions, so both refusals have to surface through the Sandbox
  # entry point unchanged — a forwarding that swallowed either would
  # leave the per-value coverage in test_sandbox_options.rb unreachable.
  def test_option_keywords_forward_to_sandbox_options_rejection
    assert_raises(ArgumentError, "an unknown profile through Sandbox.new must be rejected") do
      Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, profile: :sealed)
    end
    assert_raises(ArgumentError, "an unknown keyword through Sandbox.new must be rejected") do
      Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, bogus: 1)
    end
  end
end
