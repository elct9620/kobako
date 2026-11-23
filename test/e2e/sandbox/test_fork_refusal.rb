# frozen_string_literal: true

require "test_helper"

# E2E — a fork from a Thread inside a run, through the real Guest Binary,
# which raises, as the fork would wait on the Thread's own run.
class TestE2EForkRefusal < Minitest::Test
  include ForkingChild

  FORKS_IN_A_CALLBACK = <<~RUBY
    probe = Object.new
    probe.define_singleton_method(:kernel_fork) { fork_outcome.first }
    probe.define_singleton_method(:popen) do
      IO.popen("-") { |io| exit!(0) if io.nil? }
      :forked
    rescue RuntimeError
      :raised
    end
    probe.define_singleton_method(:daemon) do
      Process.daemon(true, true)
      :forked
    rescue RuntimeError
      :raised
    end
    probe.define_singleton_method(:kobako_error?) do
      fork { exit!(0) }
    rescue Kobako::Error
      true
    rescue RuntimeError
      false
    end
    print sandbox.bind("Probe", probe).eval("[Probe.kernel_fork, Probe.popen, Probe.daemon, Probe.kobako_error?]").value.inspect
  RUBY

  # @behavior S-238
  def test_a_fork_from_a_run_raises
    assert_equal "[:raised, :raised, :raised, false]", run_forking_child(FORKS_IN_A_CALLBACK),
                 "Kernel#fork, IO.popen(\"-\") and Process.daemon in a Service called from Sandbox#eval must each " \
                 "raise RuntimeError, which rescue Kobako::Error does not take"
  end

  FORK_BESIDE_SUSPENDED_RUN = <<~RUBY
    suspender = Object.new
    suspender.define_singleton_method(:suspend) { Fiber.yield }
    suspended = sandbox.bind("Suspender", suspender)
    fiber = Fiber.new { suspended.eval("Suspender.suspend; 1").value }
    fiber.resume
    outcome = fork_outcome.first
    print [outcome, fiber.resume].inspect
  RUBY

  # @behavior S-238
  def test_a_fork_beside_a_fiber_suspended_in_a_run_raises
    assert_equal "[:raised, 1]", run_forking_child(FORK_BESIDE_SUSPENDED_RUN),
                 "Kernel#fork beside a Fiber suspended in a Service its Sandbox#eval called must raise " \
                 "RuntimeError, and the run must answer once resumed"
  end

  FORKS_IN_A_TRAP = <<~RUBY
    print(%i[hold release].map do |gvl|
      seen = []
      ended = false
      trap("USR1") { seen << [fork_outcome.first, ended] }
      started = Queue.new
      computing = Kobako::Sandbox.new(wasm_path: WASM, gvl: gvl, timeout: 2).bind("Started", -> { started << 1 })
      Thread.new { started.pop; Process.kill("USR1", Process.pid) }
      begin
        computing.eval("Started.call; loop {}")
      rescue Kobako::TimeoutError
        ended = true
      end
      seen
    end.inspect)
  RUBY

  # @behavior S-238
  def test_a_fork_from_a_trap_handler_interrupting_a_run_raises
    assert_equal "[[[:raised, false]], [[:raised, false]]]", run_forking_child(FORKS_IN_A_TRAP),
                 "Kernel#fork in a trap handler that runs while the same Thread's Sandbox#eval (either gvl) " \
                 "computes must raise RuntimeError before the run ends"
  end
end
