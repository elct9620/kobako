# frozen_string_literal: true

require "test_helper"

# E2E — a run waiting on the host, its Thread woken by Ruby, through the real
# Guest Binary. Ruby wakes a Thread for an exception, a trap, or
# Thread#wakeup alike, so the wait goes on unless the wakeup raises.
class TestE2EWakeupDuringWait < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  # A Ruby exception outside StandardError, as a Host App stops a run with.
  class Stop < Exception; end # rubocop:disable Lint/InheritException

  LOOKUP = <<~RUBY
    http = Net::HTTP.new("stall.test", 80)
    http.open_timeout = %<open_timeout>s
    begin
      http.get("/")
    rescue Net::OpenTimeout => e
      e.class.to_s
    end
  RUBY

  # What the guest's lookup answers while the Mock holds it past
  # +open_timeout+ seconds, the run's Thread taking +poke+ 0.2 s in.
  def lookup_while(gvl, open_timeout: 0.8, &poke)
    mock = Kobako::Network::Mock.new
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: gvl, timeout: 10, network: mock)
                             .network(:web) { |net| net.endpoint "stall.test", port: 80 }
    script = format(LOOKUP, open_timeout: open_timeout)
    runner = Thread.new { mock.hold("stall.test") { sandbox.eval(script).value } }
    sleep 0.2
    poke.call(runner)
    runner.value
  end

  # What a lookup with a 5 s open_timeout raised, its Thread taking
  # Thread#raise(Stop) 0.2 s in, and how many seconds the run lasted.
  def stopped_lookup(gvl)
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    raised = begin
      lookup_while(gvl, open_timeout: 5) { |runner| runner.raise(Stop) }
    rescue Stop => e
      e
    end
    [raised, Process.clock_gettime(Process::CLOCK_MONOTONIC) - started]
  end

  # What the guest's write answers while another Sandbox's mount block holds
  # the directory, the run's Thread woken 0.2 s in.
  def write_while_held(gvl)
    tmpfs = Kobako::Filesystem::Tmpfs.new
    holder = holding(tmpfs)
    waiting = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: gvl, timeout: 10)
                             .mount("/data", tmpfs, access: :read_write)
    runner = Thread.new { waiting.eval('File.write("/data/b", "2")').value }
    sleep 0.2
    runner.wakeup
    runner.value.tap { holder.join }
  end

  # A Thread whose run holds +tmpfs+'s root for 0.6 s from its mount's
  # block, started once the block holds it.
  def holding(tmpfs)
    asking = Queue.new
    holder = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: :release)
    holder.mount("/data", tmpfs, access: :read_write) do |_path, access|
      (asking << true) && sleep(0.6) if access == :read_write
      true
    end
    Thread.new { holder.eval('File.write("/data/a", "1")') }.tap { asking.pop }
  end

  %i[hold release].each do |gvl|
    # @behavior S-222
    define_method(:"test_a_lookup_goes_on_through_a_wakeup_under_#{gvl}") do
      assert_equal "Net::OpenTimeout", lookup_while(gvl, &:wakeup),
                   "a guest's Net::HTTP lookup the Mock holds, with gvl: :#{gvl}, its Thread taking " \
                   "Thread#wakeup, must go on waiting until its own open_timeout"
    end

    # @behavior S-222
    define_method(:"test_a_directory_wait_goes_on_through_a_wakeup_under_#{gvl}") do
      assert_equal 1, write_while_held(gvl),
                   "a guest's File.write to a directory another run holds, with gvl: :#{gvl}, its Thread " \
                   "taking Thread#wakeup, must go on waiting and write once the other lets go"
    end

    # @behavior S-222
    define_method(:"test_a_wakeup_that_raises_ends_the_run_under_#{gvl}") do
      raised, spent = stopped_lookup(gvl)
      assert_equal [Stop, true], [raised.class, spent < 3],
                   "a Thread#raise(Stop) 0.2 s into a guest's Net::HTTP lookup the Mock holds with a 5 s " \
                   "open_timeout, with gvl: :#{gvl}, must end the run at once and raise Stop in the Host App, " \
                   "got #{raised.inspect} after #{spent.round(2)} s"
    end
  end

  TRAP_CHILD = <<~RUBY.freeze
    require "kobako"
    $hits = 0
    trap("USR1") { $hits += 1 }
    mock = Kobako::Network::Mock.new
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: ARGV[0].to_sym, timeout: 10, network: mock)
                             .network(:web) { |net| net.endpoint "stall.test", port: 80 }
    poker = Thread.new { sleep 0.2; Process.kill("USR1", Process.pid) }
    answer = mock.hold("stall.test") { sandbox.eval(#{format(LOOKUP, open_timeout: 0.8).dump}).value }
    poker.join
    print answer, " hits=", $hits
  RUBY

  %i[hold release].each do |gvl|
    # @behavior S-222
    define_method(:"test_a_lookup_goes_on_through_a_trap_that_only_sets_a_flag_under_#{gvl}") do
      skip "Windows has no SIGUSR1 to trap" if Gem.win_platform?
      assert_equal "Net::OpenTimeout hits=1",
                   run_ruby(TRAP_CHILD, gvl.to_s, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: 60),
                   "a guest's Net::HTTP lookup the Mock holds on the main Thread, with gvl: :#{gvl}, the process " \
                   "taking a SIGUSR1 whose trap only counts it, must run the trap once and go on waiting " \
                   "until its own open_timeout"
    end
  end
end
