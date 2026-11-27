# frozen_string_literal: true

require "test_helper"

# E2E — a run whose network lookup the Mock holds, its thread woken by Ruby,
# through the real guest binary. Ruby wakes a thread for an exception, a
# trap, or Thread#wakeup alike, so the wait goes on unless the wakeup raises.
class TestE2EWakeupDuringLookup < Minitest::Test
  include E2eGuestHelper
  include ChildProcess
  include Handshake

  # A Ruby exception outside StandardError, as a host app stops a run with.
  class Stop < Exception; end # rubocop:disable Lint/InheritException

  # The guest's own open_timeout, which ends the lookup the mock holds.
  OPEN_TIMEOUT = 0.3

  LOOKUP = <<~RUBY
    http = Net::HTTP.new("stall.test", 80)
    http.open_timeout = %<open_timeout>s
    answer = begin
      http.get("/")
    rescue Net::OpenTimeout => e
      e.class.to_s
    end
    Host::Went.call
    answer
  RUBY

  # What the guest's lookup answers while the mock holds it past
  # +open_timeout+ seconds, the run's thread taking +poke+ once the lookup
  # reached the mock; +went+ is filled once the guest goes on past it.
  def lookup_while(gvl, went = [], open_timeout: OPEN_TIMEOUT, &poke)
    mock = Kobako::Network::Mock.new
    sandbox = stalled_sandbox(gvl, mock, went)
    script = format(LOOKUP, open_timeout: open_timeout)
    runner = Thread.new { mock.hold("stall.test") { sandbox.eval(script).value } }
    runner.report_on_exception = false
    wait_until("the guest's lookup never reached the mock") { mock.lookups.any? || !runner.alive? }
    poke.call(runner)
    runner.value
  end

  def stalled_sandbox(gvl, mock, went)
    Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: gvl, timeout: 30, network: mock)
                   .network(:web) { |net| net.endpoint "stall.test", port: 80 }
                   .bind("Host::Went", -> { went << true })
  end

  # What a lookup with an open_timeout far past the test raised once its
  # thread took Thread#raise(Stop), and whether the guest went on past the
  # lookup, as it would only once the open_timeout ended it.
  def stop_lookup(gvl)
    went = []
    lookup_while(gvl, went, open_timeout: 20) { |runner| runner.raise(Stop) }
    [nil, went.any?]
  rescue Stop => e
    [e, went.any?]
  end

  %i[hold release].each do |gvl|
    # @behavior S-222
    define_method(:"test_a_lookup_goes_on_through_a_wakeup_under_#{gvl}") do
      assert_equal "Net::OpenTimeout", lookup_while(gvl, &:wakeup),
                   "a guest's Net::HTTP lookup the Mock holds, with gvl: :#{gvl}, its Thread taking " \
                   "Thread#wakeup, must go on waiting until its own open_timeout"
    end

    # @behavior S-222
    define_method(:"test_a_wakeup_that_raises_ends_the_run_under_#{gvl}") do
      raised, went = stop_lookup(gvl)
      assert_equal [Stop, false], [raised.class, went],
                   "a Thread#raise(Stop) into a guest's Net::HTTP lookup the Mock holds, with gvl: :#{gvl}, " \
                   "must end the run before the lookup's own open_timeout and raise Stop in the Host App, " \
                   "got #{raised.inspect}"
    end
  end

  TRAP_CHILD = <<~RUBY.freeze
    require "kobako"
    $hits = 0
    trap("USR1") { $hits += 1 }
    mock = Kobako::Network::Mock.new
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: ARGV[0].to_sym, timeout: 30, network: mock)
                             .network(:web) { |net| net.endpoint "stall.test", port: 80 }.bind("Host::Went", -> {})
    poker = Thread.new { sleep 0.001 while mock.lookups.empty?; Process.kill("USR1", Process.pid) }
    answer = mock.hold("stall.test") { sandbox.eval(#{format(LOOKUP, open_timeout: OPEN_TIMEOUT).dump}).value }
    poker.join
    print answer, " hits=", $hits
  RUBY

  %i[hold release].each do |gvl|
    # @behavior S-222
    define_method(:"test_a_lookup_goes_on_through_a_trap_that_only_sets_a_flag_under_#{gvl}") do
      skip "Windows has no SIGUSR1 to trap" if Gem.win_platform?
      assert_equal "Net::OpenTimeout hits=1",
                   run_ruby(TRAP_CHILD, gvl.to_s),
                   "a guest's Net::HTTP lookup the Mock holds on the main Thread, with gvl: :#{gvl}, the process " \
                   "taking a SIGUSR1 whose trap only counts it, must run the trap once and go on waiting " \
                   "until its own open_timeout"
    end
  end
end
