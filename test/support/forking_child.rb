# frozen_string_literal: true

# A child process that forks beside its own runs, through the real Guest
# Binary. A fork that waits forever is killed past BOUND rather than hanging
# the suite. The prelude gives each script a Service that blocks on a Queue,
# and the outcome of a fork as the script sees it.
module ForkingChild
  include ChildProcess

  BOUND = 30

  PRELUDE = <<~RUBY
    require "kobako"
    require "stringio"
    WASM = ENV.fetch("KOBAKO_WASM")
    Blocker = Struct.new(:queue) do
      def wait = queue.pop
    end
    def sandbox = Kobako::Sandbox.new(wasm_path: WASM)
    def child_answers?(sandbox)
      pid = fork { exit!(sandbox.eval("1 + 1").value == 2 ? 0 : 1) }
      Process.wait(pid)
      $?.success?
    end
    def fork_outcome
      pid = fork { exit!(0) }
      Process.wait(pid)
      :forked
    rescue RuntimeError => e
      [:raised, e.message]
    end
  RUBY

  def setup
    super
    skip "this platform cannot fork" unless Process.respond_to?(:fork)
  end

  # What +script+ prints, run after the prelude in a child.
  def run_forking_child(script)
    run_ruby(PRELUDE + script, env: { "KOBAKO_WASM" => E2eGuestHelper::REAL_WASM }, deadline: BOUND)
  end
end
