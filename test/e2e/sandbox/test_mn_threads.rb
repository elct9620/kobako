# frozen_string_literal: true

require "test_helper"

# E2E — runs on threads of Ruby's M:N scheduler, which carries a thread on
# another native thread once it gives the GVL up. Each guest stays on a thread
# of its own, so a service call answers wherever its caller went on. The
# scheduler is chosen as the process starts, so the case runs in a child.
class TestE2EMnThreads < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  CHILD = <<~RUBY
    require "kobako"
    answers = %i[hold release].flat_map do |gvl|
      sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: gvl)
      sandbox.bind("Host::Pass", ->(value) { Thread.pass; sleep 0.0005; value })
      Array.new(4) do
        Thread.new { Array.new(40) { |i| sandbox.eval("Host::Pass.call(\#{i}) + Host::Pass.call(1)").value } }
      end.flat_map(&:value)
    end
    print answers == Array.new(8) { (1..40).to_a }.flatten
  RUBY

  # @behavior S-228
  def test_runs_on_threads_of_rubys_mn_scheduler_answer
    skip "Ruby's M:N scheduler runs off Windows only" if Gem.win_platform?

    env = { "KOBAKO_WASM" => REAL_WASM, "RUBY_MN_THREADS" => "1" }
    assert_equal "true", run_ruby(CHILD, env: env, deadline: 60),
                 "Sandbox#eval with gvl: :hold and :release on four Threads of Ruby's M:N scheduler, each " \
                 "calling a Service that passes the Thread, must answer every run"
  end
end
