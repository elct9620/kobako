# frozen_string_literal: true

require "test_helper"

# E2E — how long a Thread waits for the GVL beside a run keeping it, against
# a Thread computing in plain Ruby, under the quantum RUBY_THREAD_TIMESLICE
# sets. A Ruby reading no such variable keeps 100 ms for both.
class TestE2EGvlTurnsQuantum < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  DEADLINE = 30

  # Prints the longest of a few waits for the GVL beside a Thread that never
  # gives it up of its own accord: a guest that never ends, or plain Ruby.
  STALL = <<~RUBY
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
    waits = Array.new(3) do
      started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      sleep 0.001
      Process.clock_gettime(Process::CLOCK_MONOTONIC) - started
    end
    print waits.max
    $stdout.flush
    exit!(0)
  RUBY

  # @behavior S-247
  def test_a_run_keeping_the_gvl_hands_it_over_at_the_quantum_rubys_threads_keep
    env = { "KOBAKO_WASM" => REAL_WASM, "RUBY_THREAD_TIMESLICE" => "1000" }
    guest, ruby = %w[guest ruby].map { |beside| run_ruby(STALL, beside, env: env, deadline: DEADLINE) }
    refute_equal :deadlocked, guest, "a Thread must get the GVL back beside a Sandbox keeping it whose guest never ends"
    assert_equal ruby.to_f > 0.5, guest.to_f > 0.5,
                 "under RUBY_THREAD_TIMESLICE=1000 a Thread must wait for the GVL beside a Sandbox keeping it " \
                 "as long as beside a Thread computing in plain Ruby; waited #{guest}s beside the guest and " \
                 "#{ruby}s beside plain Ruby"
  end
end
