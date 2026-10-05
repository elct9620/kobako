# frozen_string_literal: true

require "test_helper"

# Distinct Sandboxes on distinct Threads execute independently. The pool
# suite extends this contract to pooled checkout; this is the direct
# witness: each thread owns its Sandbox, and guest global state set on one
# never reaches the other.
class TestE2EThreading < Minitest::Test
  include E2eGuestHelper

  # @behavior RT-001
  def test_distinct_sandboxes_on_distinct_threads_execute_independently
    results = Array.new(2)
    2.times.map do |i|
      source = format("$mark ||= %<mark>d; $mark + %<add>d", mark: i, add: i * 10)
      Thread.new { results[i] = Kobako::Sandbox.new.eval(source).value }
    end.each(&:join)

    assert_equal [0, 11], results,
                 "two Sandboxes evaluated on two Threads must each return their own thread's result"
  end

  # @behavior RT-065
  def test_invocations_at_once_on_one_sandbox_each_capture_only_their_own_output
    shared = Kobako::Sandbox.new

    assert_each_captures_its_own(writing_at_once { shared }, "one shared Sandbox")
  end

  # @behavior RT-065
  def test_invocations_at_once_on_several_sandboxes_each_capture_only_their_own_output
    assert_each_captures_its_own(writing_at_once { Kobako::Sandbox.new }, "several Sandboxes")
  end

  # Each Thread writes a marker of its own many times on both channels,
  # so a capture shared between invocations would hold another's marker.
  MARKED_WRITES = "20.times { puts 'out%<i>d'; $stderr.puts 'err%<i>d' }"

  private

  def writing_at_once(&sandbox)
    Array.new(4) { |i| Thread.new { sandbox.call.eval(format(MARKED_WRITES, i: i)) } }.map(&:value)
  end

  def assert_each_captures_its_own(executions, shape)
    executions.each_with_index do |execution, i|
      seen = [execution.stdout, execution.stderr].map { |capture| capture.lines.map(&:chomp).uniq }

      assert_equal [["out#{i}"], ["err#{i}"]], seen,
                   "invocations running at once on #{shape} must each capture only what they wrote"
    end
  end
end
