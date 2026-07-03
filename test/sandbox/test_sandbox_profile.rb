# frozen_string_literal: true

require "test_helper"

# docs/behavior/security.md B-54: Sandbox.new(profile:) requests the
# isolation rung the runtime builds and declares, and construction
# enforces the request as a floor — a declaration below it fails with
# Kobako::SetupError (E-49) before any invocation entry point runs, and
# an off-ladder declaration ranks below every floor (fail-closed).
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

  # The bundled driver builds and declares whichever rung is requested,
  # so the floor check's failing branch (E-49) needs a runtime that
  # cannot honor the request — witnessed through a stubbed Runtime
  # declaring :permissive against the default :hermetic request, the
  # shape an alternative engine on the kobako-runtime contract may take.
  # Stubbed by singleton-method replacement: minitest 6 no longer
  # bundles minitest/mock.
  def test_runtime_declaring_below_the_requested_floor_fails_construction
    permissive_runtime = Object.new
    permissive_runtime.define_singleton_method(:profile) { :permissive }

    with_stubbed_from_path(permissive_runtime) do
      err = assert_raises(Kobako::SetupError, "a :permissive declaration must fail a :hermetic floor (E-49)") do
        Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
      end
      assert_match(/permissive/, err.message)
      assert_match(/hermetic/, err.message)
    end
  end

  # B-54's fail-closed clause: a declaration the gem cannot place on the
  # ladder ranks below every floor — even the weakest — so a runtime
  # declaring an unknown posture never constructs. Witnessed against the
  # :permissive floor because that is the rung an off-ladder declaration
  # could most plausibly slip past.
  def test_runtime_declaring_off_the_ladder_fails_every_floor
    unplaceable_runtime = Object.new
    unplaceable_runtime.define_singleton_method(:profile) { :isolated }

    with_stubbed_from_path(unplaceable_runtime) do
      err = assert_raises(Kobako::SetupError,
                          "an off-ladder declaration must rank below even the :permissive floor (B-54 fail-closed)") do
        Kobako::Sandbox.new(wasm_path: FIXTURE_PATH, profile: :permissive)
      end
      assert_match(/isolated/, err.message)
    end
  end

  private

  def with_stubbed_from_path(fake)
    original = Kobako::Runtime.method(:from_path)
    Kobako::Runtime.singleton_class.define_method(:from_path) { |*| fake }
    yield
  ensure
    Kobako::Runtime.singleton_class.define_method(:from_path, original)
  end
end
