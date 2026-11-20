# frozen_string_literal: true

require "test_helper"

# E2E — a SIGSEGV outside the guest after a run, through the real Guest Binary.
#
# A run leaves the process's handlers for the guest's faults in place, so a
# fault the Host App itself takes must still reach Ruby's crash report rather
# than be read as the guest's.
class TestE2EHostFault < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  BOUND = 30

  SCRIPT = <<~RUBY
    require "kobako"
    Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM")).eval("1")
    Process.kill(:SEGV, Process.pid)
    sleep 5
  RUBY

  def setup
    super
    skip "this platform has no SIGSEGV to send" unless Signal.list.key?("SEGV") && !Gem.win_platform?
  end

  # @behavior S-224
  def test_a_fault_outside_the_guest_reaches_rubys_crash_report
    report = Tempfile.create("kobako-host-fault") do |errors|
      pid = spawn_ruby(SCRIPT, env: { "KOBAKO_WASM" => REAL_WASM }, out: File::NULL, err: errors.path)
      assert finished_within?(pid, BOUND), "a process that faults after Sandbox#eval must end"
      File.read(errors.path)
    end
    refute Process.last_status.success?, "a process that faults after Sandbox#eval must not exit cleanly"
    assert_includes report, "[BUG] Segmentation fault",
                    "a SIGSEGV the Host App takes after Sandbox#eval must reach Ruby's crash report"
  end
end
