# frozen_string_literal: true

require "test_helper"

# E2E — a SIGINT a process receives during a run, through the real Guest
# Binary.
#
# The guest never checks for a Ruby interrupt, so the signal waits until the
# run hands its result back. The first result a process wraps is the one that
# first meets that waiting signal, so only a child process that has run
# nothing before can witness it. The child says when its run is about to
# start and the test signals it then; the guest loop only has to outlast that
# hand-off. The child prints what it caught, "late" when the signal missed
# the run, and anything else ends it without a word.
#
# A guest that computes without end meets the signal as its run's caller takes
# what Ruby delivered each quantum; that child says it is ready from inside its
# run, so the signal lands while the guest computes.
class TestE2EFirstRunInterrupt < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  DEADLINE = 60
  HAND_OFF = 0.05

  def setup
    super
    require_child_sigint
  end

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
    assert_equal "Interrupt", caught_interrupting(CHILD),
                 "a SIGINT during a process's first Sandbox#eval must raise Interrupt in the Host App"
  end

  COMPUTING_CHILD = <<~RUBY
    require "kobako"
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), profile: :permissive, timeout: nil)
    sandbox.bind("Host::Ready", lambda {
      $stdout.write("ready\\n")
      $stdout.flush
    })
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    begin
      sandbox.eval("Host::Ready.call; loop { }")
      print "returned"
    rescue Interrupt
      waited = Process.clock_gettime(Process::CLOCK_MONOTONIC) - started
      print(waited < 10 ? "Interrupt" : "waited")
    end
  RUBY

  # @behavior S-215
  def test_an_interrupt_delivered_while_a_guest_keeping_the_gvl_computes_stops_the_run_as_it_computes
    assert_equal "Interrupt", caught_interrupting(COMPUTING_CHILD),
                 "a SIGINT while a Sandbox#eval keeping the GVL runs `loop { }` without a deadline must raise " \
                 "Interrupt in the Host App within moments"
  end

  private

  # What +child+ prints once it is sent SIGINT after saying it is ready, or
  # +:hung+ when it outlives the deadline.
  def caught_interrupting(child)
    reader, writer = IO.pipe
    pid = spawn_ruby(child, env: { "KOBAKO_WASM" => REAL_WASM }, out: writer, err: File::NULL)
    writer.close
    interrupt_once_running(reader, pid)
    finished_within?(pid, DEADLINE) ? reader.read : :hung
  ensure
    reader&.close
  end

  def interrupt_once_running(reader, pid)
    return unless reader.wait_readable(DEADLINE) && reader.gets == "ready\n"

    sleep HAND_OFF
    Process.kill(:INT, pid)
  end
end
