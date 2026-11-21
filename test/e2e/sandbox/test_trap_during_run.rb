# frozen_string_literal: true

require "test_helper"

# E2E — a main-Thread trap handler that raises, landing as the guest
# computes. The guest stands on a thread of its own, so the handler runs on
# the caller, which holds what it raises before the next callback starts and
# raises it once the run ends. A trap handler is the process's one, so each
# case runs in a child; another process sends the signal, so it arrives while
# a run keeping the GVL holds it.
class TestE2ETrapDuringRun < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  # Prints what the run raised, whether the callback saw the handler's
  # exception, and what the next run answers.
  CHILD = <<~RUBY
    require "kobako"
    gvl, callback = ARGV.map(&:to_sym)
    seen = []
    tmpfs = Kobako::Filesystem::Tmpfs.new
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: gvl)
    sandbox.bind("Host::Pass", lambda do |value|
      value
    rescue StandardError => e
      seen << e.message
      value
    end)
    sandbox.mount("/data", tmpfs, access: :read_write) do |_path, _access|
      true
    rescue StandardError => e
      seen << e.message
      true
    end
    trap("USR1") { raise "trapped" }
    touch = callback == :mount ? 'File.write("/data/x", "1")' : "Host::Pass.call(i)"
    Process.detach(spawn("sleep 0.1; kill -USR1 \#{Process.pid}"))
    raised = begin
      sandbox.eval("i = 0; i += 1 while i < 10_000_000; \#{touch}")
      nil
    rescue RuntimeError => e
      e.message
    end
    print [raised, seen, sandbox.eval("Host::Pass.call(2)").value].inspect
  RUBY

  # @behavior S-227
  def test_a_trap_that_raises_as_the_guest_computes_ends_that_run
    skip "Windows has no SIGUSR1" if Gem.win_platform?

    %i[hold release].each do |gvl|
      assert_equal '["trapped", [], 2]', run_child(gvl, :service),
                   "Sandbox#eval (gvl: #{gvl}) on the main Thread, whose trap handler raises as the guest " \
                   "computes, must raise the handler's exception once the run ends, never reach the Service, " \
                   "and let the next run answer"
    end
  end

  # @behavior S-229
  def test_a_trap_that_raises_as_the_guest_computes_never_reaches_a_mounts_block
    skip "Windows has no SIGUSR1" if Gem.win_platform?

    %i[hold release].each do |gvl|
      assert_equal '["trapped", [], 2]', run_child(gvl, :mount),
                   "Sandbox#eval (gvl: #{gvl}) on the main Thread, whose trap handler raises as the guest " \
                   "computes before it opens a mounted file, must raise the handler's exception once the run " \
                   "ends, and the mount's block that rescues every error must never see it"
    end
  end

  private

  def run_child(gvl, callback)
    run_ruby(CHILD, gvl.to_s, callback.to_s, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: 30)
  end
end
