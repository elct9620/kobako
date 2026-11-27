# frozen_string_literal: true

require "test_helper"
require "tmpdir"

# E2E — a SIGINT a process receives during a run, through the real Guest
# Binary.
#
# The run's caller takes what Ruby delivered each quantum it waits for its
# guest, so the signal stops a guest that computes without end, in the first
# run a process makes as in any other. Only a child process that has run
# nothing before can witness the first. Its guest writes a file on a Bind
# mount, which runs no Ruby, as it starts computing; the test sends the
# signal once the file is there, so the signal lands while the guest computes
# and never inside a callback. A guest that never ends on its own means only
# the signal ends the run, and the child's lifetime ends a run it never
# reached.
class TestE2EFirstRunInterrupt < Minitest::Test
  include E2eGuestHelper
  include ChildProcess
  include Handshake

  def setup
    super
    require_child_sigint
  end

  CHILD = <<~RUBY
    require "kobako"
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), timeout: nil)
                             .mount("/t", Kobako::Filesystem::Bind.new(ENV.fetch("GATE")), access: :read_write)
    begin
      sandbox.eval('File.write("/t/ready", "1"); loop { }')
      print "returned"
    rescue Interrupt
      print "Interrupt"
    end
  RUBY

  # @behavior S-206
  def test_an_interrupt_during_a_processs_first_run_reaches_the_host_app_as_itself
    assert_equal "Interrupt", interrupt_child(CHILD),
                 "a SIGINT during a process's first Sandbox#eval must raise Interrupt in the Host App"
  end

  COMPUTING_CHILD = <<~RUBY
    require "kobako"
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), profile: :permissive, timeout: nil)
                             .mount("/t", Kobako::Filesystem::Bind.new(ENV.fetch("GATE")), access: :read_write)
    begin
      sandbox.eval('File.write("/t/ready", "1"); loop { }')
      print "returned"
    rescue Interrupt
      print "Interrupt"
    end
  RUBY

  # @behavior S-215
  def test_an_interrupt_delivered_while_a_guest_keeping_the_gvl_computes_stops_the_run_as_it_computes
    assert_equal "Interrupt", interrupt_child(COMPUTING_CHILD),
                 "a SIGINT while a Sandbox#eval keeping the GVL runs `loop { }` without a deadline must raise " \
                 "Interrupt in the Host App"
  end

  private

  # What +child+ prints once it is sent SIGINT after its guest started
  # computing, or +:hung+ when it outlives its lifetime.
  def interrupt_child(child)
    Dir.mktmpdir("kobako-interrupt") do |gate|
      reader, writer = IO.pipe
      pid = spawn_ruby(child, env: { "GATE" => gate }, out: writer, err: File::NULL)
      writer.close
      interrupt_once_computing(gate, pid)
      finished_within?(pid) ? reader.read : :hung
    ensure
      reader&.close
    end
  end

  def interrupt_once_computing(gate, pid)
    wait_until("the child's guest never started computing, so its run failed first") do
      File.exist?(File.join(gate, "ready"))
    end
    Process.kill(:INT, pid)
  end
end
