# frozen_string_literal: true

require "test_helper"

# Sandbox.new(profile:) requests the isolation profile the runtime builds
# and declares, and Sandbox.new(gvl:) the mode it runs under; the runtime
# judges both as it is built. The floor's failing branch is witnessed by
# the Rust SDK, which can hand a Sandbox a runtime of its own: the bundled
# runtime always builds the requested profile.
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

  # @behavior RT-033
  # nil included deliberately: the no-floor request is an explicit
  # :permissive, so profile has no nil-disable form.
  def test_a_profile_kobako_does_not_name_is_refused
    [nil, :sealed, "hermetic", 1].each do |bad|
      assert_raises(ArgumentError, "profile #{bad.inspect} through Sandbox.new must be refused") do
        Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, profile: bad)
      end
    end
  end

  # @behavior RT-020
  # nil included deliberately: gvl is requested as an explicit mode, so it
  # has no nil-disable form.
  def test_a_mode_outside_the_set_is_refused
    [nil, :auto, "release", 1].each do |bad|
      assert_raises(ArgumentError, "gvl #{bad.inspect} through Sandbox.new must be refused") do
        Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, gvl: bad)
      end
    end
  end

  # @behavior RT-034
  def test_a_keyword_the_options_do_not_take_is_refused
    assert_raises(ArgumentError, "an unknown keyword through Sandbox.new must be refused") do
      Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, bogus: 1)
    end
  end
end
