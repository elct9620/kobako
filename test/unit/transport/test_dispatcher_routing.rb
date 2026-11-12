# frozen_string_literal: true

require "test_helper"

# Unit-level coverage of Transport::Dispatcher's routing: a dispatch is
# routed by its target and method alone, before its arguments are read.
class TestTransportDispatchRouting < Minitest::Test
  include DispatcherHelpers

  # Arguments nested past the codec's depth bound, which no decode reads.
  UNREADABLE = "\x92#{"\x91" * 1000}\xC0\x80".b

  def setup
    super
    @registry.bind("Logger::Echo", ->(arg) { arg })
  end

  # @behavior T-390
  def test_a_dispatch_it_cannot_route_answers_undefined_before_reading_the_arguments
    answers = [%w[Missing::Service call], %w[Logger::Echo instance_eval]].map do |target, method|
      call = Kobako::Transport::Call.new(target: target, method_name: method,
                                         block_given: false, payload: UNREADABLE)
      DispatcherHelpers.reify(dispatch(call)).payload.type
    end

    assert_equal %w[undefined undefined], answers,
                 "a dispatch to an unbound Service, or to a method the Service does not expose, " \
                 "must answer undefined through Dispatcher.dispatch whatever its arguments hold, " \
                 "since routing reads only the target and the method"
  end
end
