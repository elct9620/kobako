# frozen_string_literal: true

require "test_helper"

# E2E — a block exit unwinding a Service the way Ruby unwinds a plain
# method: the Service's ensure runs, its rescue cannot stop the exit, and
# a yield from that ensure runs the block before the exit goes on.
class TestE2EYieldExit < Minitest::Test
  include E2eGuestHelper

  # Records that its ensure clause ran.
  class Guarded
    attr_reader :cleaned

    def run
      yield
    ensure
      @cleaned = true
    end
  end

  # Guards its yield with a bare rescue.
  class Rescuing
    def run
      yield
    rescue StandardError
      :rescued
    end
  end

  # Yields once more from its ensure clause and keeps what that answered.
  class Reyielding
    attr_reader :second

    def run
      yield 1
    ensure
      @second = yield 2
    end
  end

  # The Service's own ensure is its clean-up, so a block exit unwinding it
  # must run that clause as Ruby runs it for a plain method.
  # @behavior T-293
  def test_a_block_exit_runs_the_services_ensure
    service = Guarded.new

    result = eval_with(service, "Probe::Service.run { break :out }")

    assert_equal [:out, true], [result, service.cleaned],
                 "a `break` through Sandbox#eval must run the Service's ensure clause " \
                 "before the call answers the break value"
  end

  # A bare rescue is how a Service guards its yield, and it still cannot
  # stop a `break`, so the guest's exit is never swallowed.
  # @behavior T-294
  def test_a_service_cannot_rescue_a_block_exit
    result = eval_with(Rescuing.new, "Probe::Service.run { break :out }")

    assert_equal :out, result,
                 "a `break` through Sandbox#eval must answer its value even past a Service " \
                 "whose bare rescue surrounds its yield"
  end

  # The guest keeps the first exit while the ensure runs the block again;
  # a block that answers there must leave that exit in place, as plain
  # mruby and CRuby both do.
  # @behavior T-295
  def test_a_yield_from_the_ensure_runs_the_block_before_the_exit_goes_on
    service = Reyielding.new

    result = eval_with(service, "Probe::Service.run { |x| break :first if x == 1; :second }")

    assert_equal %i[first second], [result, service.second],
                 "a yield from the ensure of a Service a `break` unwinds must run the block, " \
                 "and the call through Sandbox#eval must still answer the first break's value"
  end

  private

  def eval_with(service, script)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Service", service)
    sandbox.eval(script).value
  end
end
