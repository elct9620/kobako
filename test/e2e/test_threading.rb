# frozen_string_literal: true

require "test_helper"

# Distinct Sandboxes on distinct Threads execute independently
# (docs/behavior/runtime.md B-22). The pool suite extends this contract
# to pooled checkout (B-47); this is the direct witness: each thread
# owns its Sandbox, and guest global state set on one never reaches the
# other.
class TestE2EThreading < Minitest::Test
  include E2eGuestHelper

  def test_b22_distinct_sandboxes_on_distinct_threads_execute_independently
    results = Array.new(2)
    2.times.map do |i|
      source = format("$mark ||= %<mark>d; $mark + %<add>d", mark: i, add: i * 10)
      Thread.new { results[i] = Kobako::Sandbox.new.eval(source) }
    end.each(&:join)

    assert_equal [0, 11], results,
                 "two Sandboxes evaluated on two Threads must each return their own thread's result"
  end
end
