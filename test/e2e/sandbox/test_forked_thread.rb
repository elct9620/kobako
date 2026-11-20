# frozen_string_literal: true

require "test_helper"

# E2E — a run on a Thread a forked child starts, through the real Guest Binary.
#
# The parent runs once before forking, so the process has already set up what
# catches a guest's faults when the child starts. A child that cannot run
# prints what the run raised; one that never answers is killed past the
# test's own bound.
class TestE2EForkedThread < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  BOUND = 30

  SCRIPT = <<~RUBY
    require "kobako"
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"))
    sandbox.eval("1")
    pid = fork do
      begin
        print Thread.new { sandbox.eval("1 + 1").value }.value
      rescue Exception => e
        print e.class
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

  # @behavior S-223
  def test_a_forked_child_runs_a_sandbox_on_a_thread_it_starts
    assert_equal "2", run_ruby(SCRIPT, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: BOUND),
                 "Sandbox#eval(\"1 + 1\") on a Thread a child forked after the parent ran it must answer 2"
  end
end
