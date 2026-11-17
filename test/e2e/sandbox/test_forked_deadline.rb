# frozen_string_literal: true

require "test_helper"

# E2E — a deadline in a child forked after its parent ran a Sandbox, through
# the real Guest Binary.
#
# The parent runs once before forking, so whatever keeps time for the process
# already exists when the child starts. The child runs a loop without end on
# the parent's Sandbox and prints what ended it; a child the deadline never
# reaches is killed past the test's own bound.
class TestE2EForkedDeadline < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  BOUND = 30

  SCRIPT = <<~RUBY
    require "kobako"
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), timeout: 0.5)
    sandbox.eval("1")
    pid = fork do
      begin
        sandbox.eval("loop {}")
        print "returned"
      rescue Kobako::TimeoutError
        print "TimeoutError"
      end
      $stdout.flush
      exit!(0)
    end
    Process.wait(pid)
  RUBY

  def setup
    super
    skip "this platform cannot fork" unless Process.respond_to?(:fork)
  end

  # @behavior S-216
  def test_a_deadline_cuts_a_forked_childs_endless_loop_short
    assert_equal "TimeoutError", run_ruby(SCRIPT, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: BOUND),
                 "a Sandbox#eval with timeout: 0.5 in a child forked after the parent ran it must raise " \
                 "Kobako::TimeoutError rather than loop forever"
  end
end
