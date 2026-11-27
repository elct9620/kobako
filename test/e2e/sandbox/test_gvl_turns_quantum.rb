# frozen_string_literal: true

require "test_helper"

# E2E — how long a thread waits for the GVL beside a run keeping it, against
# a thread computing in plain Ruby, under the quantum RUBY_THREAD_TIMESLICE
# sets. A Ruby reading no such variable keeps 100 ms for both.
class TestE2EGvlTurnsQuantum < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  # Longer than a 1 ms sleep takes with the GVL free, and shorter than any
  # quantum a Ruby keeps.
  FREE = 0.05
  # How long the waits go on while each still finds the GVL free.
  LONGEST = 2

  # Prints the longest wait for the GVL beside a thread that never gives it
  # up of its own accord: a guest that never ends, or plain Ruby. The file
  # stands before the guest's write returns, and the GVL is free until it
  # does, so the waits go on until one is past FREE or LONGEST has gone by.
  STALL = <<~RUBY.freeze
    require "kobako"
    require "tmpdir"
    dir = Dir.mktmpdir
    if ARGV[0] == "ruby"
      Thread.new { File.write(File.join(dir, "in"), "1"); loop { } }
    else
      sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :hold, timeout: nil)
                               .mount("/t", Kobako::Filesystem::Bind.new(dir), access: :read_write)
      Thread.new { sandbox.eval('File.write("/t/in", "1"); loop { }') }
    end
    sleep 0.01 until File.exist?(File.join(dir, "in"))
    clock = -> { Process.clock_gettime(Process::CLOCK_MONOTONIC) }
    until_at = clock.call + #{LONGEST}
    longest = 0.0
    while longest <= #{FREE} && clock.call < until_at
      started = clock.call
      sleep 0.001
      longest = [longest, clock.call - started].max
    end
    print longest
    $stdout.flush
    exit!(0)
  RUBY

  # @behavior S-247
  def test_a_run_keeping_the_gvl_hands_it_over_at_the_quantum_rubys_threads_keep
    guest, ruby = %w[guest ruby].map { |beside| run_ruby(STALL, beside, env: { "RUBY_THREAD_TIMESLICE" => "500" }) }
    refute_equal :deadlocked, guest, "a Thread must get the GVL back beside a Sandbox keeping it whose guest never ends"
    assert_equal ruby.to_f > 0.3, guest.to_f > 0.3,
                 "under RUBY_THREAD_TIMESLICE=500 a Thread must wait for the GVL beside a Sandbox keeping it " \
                 "as long as beside a Thread computing in plain Ruby; waited #{guest}s beside the guest and " \
                 "#{ruby}s beside plain Ruby"
  end
end
