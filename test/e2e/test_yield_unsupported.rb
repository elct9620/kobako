# frozen_string_literal: true

require "test_helper"

# The real Guest Binary serves every block, so a hand-written guest answers
# the host's yield as unsupported and the Service records what its yield
# raised.
class TestYieldUnsupported < Minitest::Test
  include GuestGuard

  FIXTURE_PATH = TestPaths.fixture("minimal_unsupported_guest.wat")

  # Yields once and keeps whatever the yield raised.
  class Recorder
    attr_reader :failure

    def each
      yield
    rescue Kobako::Error => e
      @failure = e
      nil
    end
  end

  def setup
    require_fixture!(FIXTURE_PATH)
  end

  # @behavior T-278
  def test_a_yield_the_guest_does_not_serve_raises_at_the_services_yield
    recorder = Recorder.new
    sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    sandbox.bind("S", recorder)

    sandbox.run(:Anything)

    assert_kind_of Kobako::UnsupportedError, recorder.failure,
                   "a yield answered as unsupported through Sandbox#run must raise at the " \
                   "Service's yield rather than trap"
  end
end
