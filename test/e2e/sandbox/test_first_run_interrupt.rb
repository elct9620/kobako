# frozen_string_literal: true

require "test_helper"
require "rbconfig"

# E2E — a SIGINT a process receives during its first run, through the real
# Guest Binary.
#
# The guest never checks for a Ruby interrupt, so the signal waits until the
# run hands its result back. The first result a process wraps is the one that
# first meets that waiting signal, so only a child process that has run
# nothing before can witness it. The child rescues the run and prints what it
# caught; anything else ends it without a word.
class TestE2EFirstRunInterrupt < Minitest::Test
  include E2eGuestHelper

  DEADLINE = 60

  CHILD = <<~RUBY
    require "kobako"
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"))
    Process.spawn("sleep 0.5 && kill -INT \#{Process.pid}")
    begin
      sandbox.eval("i = 0; while i < 100_000_000; i += 1; end; i")
      print "returned"
    rescue Interrupt
      print "Interrupt"
    end
  RUBY

  # @behavior S-206
  def test_an_interrupt_during_a_processs_first_run_reaches_the_host_app_as_itself
    reader, writer = IO.pipe
    pid = Process.spawn({ "KOBAKO_WASM" => REAL_WASM }, RbConfig.ruby, "-I", TestPaths.source("lib"),
                        "-e", CHILD, out: writer, err: File::NULL)
    writer.close
    caught = finished_within_deadline?(pid) ? reader.read : :hung

    assert_equal "Interrupt", caught,
                 "a SIGINT during a process's first Sandbox#eval must raise Interrupt in the Host App"
  ensure
    reader&.close
  end

  private

  def finished_within_deadline?(pid)
    deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + DEADLINE
    until Process.wait(pid, Process::WNOHANG)
      next sleep(0.05) if Process.clock_gettime(Process::CLOCK_MONOTONIC) < deadline

      Process.kill(:KILL, pid)
      Process.wait(pid)
      return false
    end
    true
  end
end
