# frozen_string_literal: true

require "test_helper"
require "rbconfig"

# E2E — a SIGINT a process receives during its first run, through the real
# Guest Binary.
#
# The guest never checks for a Ruby interrupt, so the signal waits until the
# run hands its result back. The first result a process wraps is the one that
# first meets that waiting signal, so only a child process that has run
# nothing before can witness it. The child says when its run is about to
# start and the test signals it then; the guest loop only has to outlast that
# hand-off. The child prints what it caught, "late" when the signal missed
# the run, and anything else ends it without a word.
class TestE2EFirstRunInterrupt < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  DEADLINE = 60
  HAND_OFF = 0.05

  CHILD = <<~RUBY
    require "kobako"
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"))
    returned = false
    begin
      $stdout.write("ready\\n")
      $stdout.flush
      sandbox.eval("i = 0; while i < 10_000_000; i += 1; end; i")
      returned = true
      sleep 1
      print "returned"
    rescue Interrupt
      print(returned ? "late" : "Interrupt")
    end
  RUBY

  # @behavior S-206
  def test_an_interrupt_during_a_processs_first_run_reaches_the_host_app_as_itself
    reader, writer = IO.pipe
    pid = Process.spawn({ "KOBAKO_WASM" => REAL_WASM }, RbConfig.ruby, "-I", TestPaths.source("lib"),
                        "-e", CHILD, out: writer, err: File::NULL)
    writer.close
    interrupt_once_running(reader, pid)
    caught = finished_within?(pid, DEADLINE) ? reader.read : :hung

    assert_equal "Interrupt", caught,
                 "a SIGINT during a process's first Sandbox#eval must raise Interrupt in the Host App"
  ensure
    reader&.close
  end

  private

  def interrupt_once_running(reader, pid)
    return unless reader.wait_readable(DEADLINE) && reader.gets == "ready\n"

    sleep HAND_OFF
    Process.kill(:INT, pid)
  end
end
