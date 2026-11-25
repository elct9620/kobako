# frozen_string_literal: true

require "test_helper"

# E2E — a fork beside another thread's run, through the real guest binary,
# which raises at once rather than wait for the run. Each script lets its runs
# go whatever the fork answered, so a fork that waits shows as unanswered
# instead of keeping the run it waits on.
class TestE2EForkBesideARun < Minitest::Test
  include ForkingChild

  BESIDE_A_WAITING_RUN = <<~RUBY
    print(%i[hold release].map do |gvl|
      queue = Queue.new
      blocked = Kobako::Sandbox.new(wasm_path: WASM, gvl: gvl).bind("Blocker", Blocker.new(queue))
      run = Thread.new { blocked.eval("Blocker.wait; :ended").value }
      Thread.pass until run.status == "sleep"
      outcome = fork_within(2)
      queue << 1
      [Array(outcome).first, run.value, child_answers?(blocked)]
    end.inspect)
  RUBY

  # @behavior S-236
  def test_a_fork_beside_another_threads_run_in_a_service_raises
    assert_equal "[[:raised, :ended, true], [:raised, :ended, true]]", run_forking_child(BESIDE_A_WAITING_RUN),
                 "Kernel#fork while another Thread's Sandbox#eval (either gvl) waits in a Service must raise " \
                 "RuntimeError at once, the run must answer, and a fork after it must leave a child whose " \
                 "Sandbox answers"
  end

  # The run is stopped once the fork answered, so it never waits out its
  # deadline, which only ends a run the stop never reached.
  BESIDE_A_COMPUTING_RUN = <<~RUBY.freeze
    Stop = Class.new(StandardError)
    print(%i[hold release].map do |gvl|
      started = Queue.new
      computing = Kobako::Sandbox.new(wasm_path: WASM, gvl: gvl, timeout: #{BOUND / 2}).bind("Started", -> { started << 1 })
      run = Thread.new do
        computing.eval("Started.call; loop {}")
      rescue Stop, Kobako::TimeoutError => e
        e.class
      end
      started.pop
      outcome = fork_within(2)
      going = run.alive?
      run.raise(Stop)
      [Array(outcome).first, going, run.value]
    end.inspect)
  RUBY

  # @behavior S-236
  def test_a_fork_beside_another_threads_computing_run_raises
    assert_equal "[[:raised, true, Stop], [:raised, true, Stop]]",
                 run_forking_child(BESIDE_A_COMPUTING_RUN),
                 "Kernel#fork while another Thread's Sandbox#eval (either gvl) computes must raise RuntimeError " \
                 "before that run ends"
  end

  BESIDE_OVERLAPPING_RUNS = <<~RUBY
    napper = Object.new
    napper.define_singleton_method(:nap) { sleep 0.02 }
    print({ "Napper.nap" => "waiting", "i = 0; i += 1 while i < 20_000" => "computing" }.map do |script, kind|
      stop = false
      going = Queue.new
      runners = Array.new(4) do
        sandbox = Kobako::Sandbox.new(wasm_path: WASM).bind("Napper", napper)
        Thread.new { sandbox.eval(script).then { going << true }; sandbox.eval(script) until stop }
      end
      4.times { going.pop }
      outcome = fork_within(2)
      stop = true
      runners.each(&:join)
      [kind, outcome != :unanswered]
    end.inspect)
  RUBY

  # @behavior S-236
  def test_a_fork_beside_runs_that_keep_overlapping_answers_at_once
    assert_equal '[["waiting", true], ["computing", true]]', run_forking_child(BESIDE_OVERLAPPING_RUNS),
                 "Kernel#fork while four Threads keep running Sandbox#eval back to back, each waiting in a " \
                 "Service or computing, must answer within two seconds"
  end

  JOINING_THE_FORKING_THREAD = <<~RUBY
    joiner = Object.new
    joiner.define_singleton_method(:join) { Thread.new { fork { exit!(0) } }.join }
    joining = Kobako::Sandbox.new(wasm_path: WASM, timeout: 1).bind("Joiner", joiner)
    run = Thread.new do
      joining.eval("Joiner.join")
    rescue Kobako::Error => e
      e.class
    end
    print((run.join(3) ? run.value : :unanswered).inspect)
    $stdout.flush
    exit!(0)
  RUBY

  # @behavior S-236
  def test_a_run_whose_service_joins_a_forking_thread_answers
    assert_equal "Kobako::ServiceError", run_forking_child(JOINING_THE_FORKING_THREAD),
                 "a Sandbox#eval whose Service joins a Thread that calls Kernel#fork must fail with " \
                 "Kobako::ServiceError within three seconds, the fork having raised in that Thread"
  end
end
