# frozen_string_literal: true

require "test_helper"

# E2E — what a Host App's callback raises beyond StandardError, through
# the real Guest Binary. Each script rescues every failure it can, then
# calls Host::Mark, so a run that went on after the callback shows in
# +went_on+ even though the run still raises in the end.
class TestE2EHostException < Minitest::Test
  include E2eGuestHelper
  include Handshake

  RESCUING_OPEN = 'begin; File.read("/data/note.txt"); rescue Exception; end; Host::Mark.call'
  RESCUING_STOP = "begin; Host::Stop.call; rescue Exception; end; Host::Mark.call"

  # A Ruby Filesystem whose open raises what it is handed.
  class RaisingFilesystem
    def initialize(&open) = @open = open
    def open(_path, _flags) = @open.call
    def stat(_path) = File.stat(__FILE__)
  end

  def setup
    @went_on = []
  end

  # @behavior RF-014
  def test_an_interrupt_a_ruby_filesystem_raises_ends_the_run
    sandbox = marking_sandbox.mount("/data", RaisingFilesystem.new { raise Interrupt })

    assert_raises(Interrupt,
                  "an Interrupt from a Ruby Filesystem's open through Sandbox#eval must raise in the Host App") do
      sandbox.eval(RESCUING_OPEN)
    end
    assert_empty @went_on, "an Interrupt from a Ruby Filesystem's open through Sandbox#eval must end the run"
  end

  # Raised into the run's thread, outside StandardError, as a Timeout or a
  # signal raises its own, once the filesystem's open has begun, where a guest
  # answer would swallow it.
  # @behavior RF-014
  def test_an_exception_raised_into_a_ruby_filesystems_open_reaches_the_host_app
    entered = Queue.new
    sandbox = marking_sandbox.mount("/data", RaisingFilesystem.new { wait_in_open(entered) })
    run = Thread.new { sandbox.eval(RESCUING_OPEN) }.tap { |thread| thread.report_on_exception = false }
    wait_until("the guest never reached the Ruby Filesystem's open") { !entered.empty? }
    run.raise(Interrupt)

    assert_raises(Interrupt,
                  "an exception raised into a Ruby Filesystem's open through Sandbox#eval must reach the Host App") do
      run.value
    end
    assert_empty @went_on, "an exception raised into a Ruby Filesystem's open through Sandbox#eval must end the run"
  end

  # @behavior RF-014
  def test_a_throw_from_a_ruby_filesystem_reaches_its_catch
    sandbox = marking_sandbox.mount("/data", RaisingFilesystem.new { throw :host, :caught })

    assert_equal :caught, catch(:host) { sandbox.eval(RESCUING_OPEN) },
                 "a throw from a Ruby Filesystem's open through Sandbox#eval must reach the Host App's catch"
  end

  # The file is there, so the open reaches the block rather than answering
  # ENOENT before any access is judged.
  # @behavior MT-023
  def test_an_interrupt_a_mount_block_raises_ends_the_run
    sandbox = marking_sandbox.mount("/data", Kobako::Filesystem::Tmpfs.new("note.txt" => "")) { raise Interrupt }

    assert_raises(Interrupt,
                  "an Interrupt from a mount's block through Sandbox#eval must raise in the Host App") do
      sandbox.eval(RESCUING_OPEN)
    end
    assert_empty @went_on, "an Interrupt from a mount's block through Sandbox#eval must end the run"
  end

  # @behavior T-282
  def test_an_interrupt_a_service_raises_ends_the_run
    sandbox = marking_sandbox
    sandbox.bind("Host::Stop", -> { raise Interrupt })

    assert_raises(Interrupt,
                  "an Interrupt from a Service through Sandbox#eval must raise in the Host App") do
      sandbox.eval(RESCUING_STOP)
    end
    assert_empty @went_on, "an Interrupt from a Service through Sandbox#eval must end the run"
  end

  # A Sandbox the host abandoned once must not stay abandoned.
  # @behavior T-282
  def test_a_sandbox_runs_again_after_the_host_abandoned_a_run
    sandbox = marking_sandbox
    sandbox.bind("Host::Stop", -> { raise Interrupt })
    assert_raises(Interrupt) { sandbox.eval("Host::Stop.call") }

    assert_equal 2, sandbox.eval("1 + 1").value,
                 "Sandbox#eval after a run a Service's Interrupt ended must run the next script"
  end

  private

  # Say on +entered+ that the open began, then wait for what is raised into it.
  def wait_in_open(entered)
    entered << true
    sleep
  end

  def marking_sandbox
    went_on = @went_on
    Kobako::Sandbox.new(wasm_path: REAL_WASM).tap { |sandbox| sandbox.bind("Host::Mark", -> { went_on << true }) }
  end
end
