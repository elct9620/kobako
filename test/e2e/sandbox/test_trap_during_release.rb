# frozen_string_literal: true

require "test_helper"

# E2E — a main-Thread trap handler that raises, landing as a guest releasing
# the GVL computes. The guest stands on a thread of its own, so the handler
# runs on the caller, where the run holds what it raises and raises it once it
# ends. A trap handler is the process's one, so the case runs in a child.
class TestE2ETrapDuringRelease < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  CHILD = <<~RUBY
    require "kobako"
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :release)
    sandbox.bind("Host::Pass", ->(value) { value })
    trap("USR1") { raise "trapped" }
    Thread.new { sleep 0.05; Process.kill("USR1", Process.pid) }
    raised = begin
      sandbox.eval("i = 0; i += 1 while i < 20_000_000; Host::Pass.call(i)")
      nil
    rescue RuntimeError => e
      e.message
    end
    print [raised, sandbox.eval("Host::Pass.call(2)").value].inspect
  RUBY

  # @behavior S-227
  def test_a_trap_that_raises_during_a_run_releasing_the_gvl_ends_that_run
    skip "Windows has no SIGUSR1" if Gem.win_platform?

    assert_equal '["trapped", 2]', run_ruby(CHILD, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: 30),
                 "Sandbox#eval with gvl: :release on the main Thread, whose trap handler raises as the guest " \
                 "computes, must raise the handler's exception once the run ends, and the next run must answer"
  end
end
