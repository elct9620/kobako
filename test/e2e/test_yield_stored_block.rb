# frozen_string_literal: true

require "test_helper"

# E2E — a block a Service stored from one Call and runs inside a later
# Call that is still open. Each yield names the Call whose block it runs,
# so the stored block runs as itself rather than as the later Call's.
class TestE2EYieldStoredBlock < Minitest::Test
  include E2eGuestHelper

  # Stores the block +wrap+ was given, then yields to it; +fire+ runs the
  # stored block from inside its own Call.
  class Relay
    def wrap(&block)
      @block = block
      yield(1)
    end

    def fire(&own)
      [@block.call(2), own&.call(3)]
    end
  end

  # The later Call carries its own block, so a yield that picks the
  # innermost block instead of the named one shows as the wrong answer.
  # @behavior T-283
  def test_a_stored_block_called_inside_a_later_call_runs_as_itself
    result = relay_sandbox.eval(<<~RUBY).value
      S.wrap { |x| x == 1 ? S.fire { |y| [:later, y] } : [:stored, x] }
    RUBY

    assert_equal [[:stored, 2], [:later, 3]], result,
                 "a stored block called inside a later Call through Sandbox#eval must run as " \
                 "itself, and the later Call's own block must still run as its own"
  end

  # @behavior T-284
  def test_a_stored_blocks_unrescued_raise_reaches_the_guest_as_itself
    result = relay_sandbox.eval(<<~RUBY).value
      begin
        S.wrap { |x| x == 1 ? S.fire : raise(ArgumentError, "stored") }
      rescue => e
        [e.class.to_s, e.message]
      end
    RUBY

    assert_equal %w[ArgumentError stored], result,
                 "a stored block's raise that neither Service rescues must reach guest code " \
                 "through Sandbox#eval as the exception the block raised"
  end

  # Had either Service finished, +wrap+ would answer +fire+'s pair rather
  # than the break value.
  # @behavior T-285
  def test_a_break_from_a_stored_block_inside_a_later_call_ends_the_first_call
    result = relay_sandbox.eval("S.wrap { |x| x == 1 ? S.fire : (break :out) }").value

    assert_equal :out, result,
                 "a break from a stored block run inside a later Call through Sandbox#eval " \
                 "must end the Call that carried the block with the break value"
  end

  # @behavior T-292
  def test_a_return_from_a_stored_block_inside_a_later_call_leaves_the_method
    result = relay_sandbox.eval(<<~RUBY).value
      def around = (S.wrap { |x| x == 1 ? S.fire : (return :out) }; :fell_through)
      around
    RUBY

    assert_equal :out, result,
                 "a return from a stored block run inside a later Call through Sandbox#eval " \
                 "must leave the method around the first Call with its value"
  end

  private

  def relay_sandbox
    Kobako::Sandbox.new(wasm_path: REAL_WASM).tap { |sandbox| sandbox.bind("S", Relay.new) }
  end
end
