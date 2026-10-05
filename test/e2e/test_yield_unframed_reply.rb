# frozen_string_literal: true

require "test_helper"

# The real Guest Binary always frames its Yield Reply, so a hand-written
# guest stages the two ways one can fail to frame — no bytes at all, and a
# tag outside the live set — and the Service records what its yield raised.
class TestYieldUnframedReply < Minitest::Test
  include GuestGuard

  FIXTURE_PATH = TestPaths.fixture("minimal_unframed_yield.wat")

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
    @recorder = Recorder.new
    @sandbox = Kobako::Sandbox.new(wasm_path: FIXTURE_PATH)
    @sandbox.bind("S", @recorder)
  end

  # @behavior T-250
  def test_an_empty_yield_answer_fails_the_services_yield_as_a_trap
    @sandbox.eval("")

    assert_kind_of Kobako::TrapError, @recorder.failure,
                   "a Yield Reply of no bytes through Sandbox#eval must fail the Service's yield as a trap"
  end

  # @behavior T-250
  def test_a_yield_answer_with_an_unknown_tag_fails_the_services_yield_as_a_trap
    @sandbox.run(:Anything)

    assert_kind_of Kobako::TrapError, @recorder.failure,
                   "a Yield Reply tagged outside the live set through Sandbox#run must fail the " \
                   "Service's yield as a trap"
  end
end
