# frozen_string_literal: true

require "test_helper"

# E2E — a sandbox the host has no thread or address space left for, through
# the real guest binary. Each case runs beside a run waiting in a service, so
# a run that fails leaves its neighbour to answer.
class TestE2EExhaustedHost < Minitest::Test
  include ExhaustedChild

  NO_THREAD_FOR_A_RUN = <<~RUBY
    queue = Queue.new
    sandbox = Kobako::Sandbox.new(wasm_path: WASM, gvl: %<gvl>s).bind("Blocker", Blocker.new(queue))
    neighbour = Thread.new { sandbox.eval("Blocker.wait; :ended").value }
    Thread.pass until neighbour.status == "sleep"
    refused, failed = without_threads { outcome(sandbox, "1") }
    queue << 1
    print [refused, failed, neighbour.value, outcome(sandbox, "2")].inspect
  RUBY

  # @behavior S-253
  def test_a_run_the_host_has_no_thread_for_raises_a_trap
    %i[hold release].each do |gvl|
      answer = run_exhausted_child(format(NO_THREAD_FOR_A_RUN, gvl: gvl.inspect))
      assert_equal "[true, Kobako::TrapError, :ended, 2]", answer,
                   "Sandbox#eval (gvl: #{gvl}) in a process whose RLIMIT_NPROC refuses Thread.new, its one guest " \
                   "thread busy, must raise Kobako::TrapError, and the waiting run and a later run must answer"
    end
  end

  NO_THREAD_FOR_THE_FIRST_SANDBOX = <<~RUBY
    refused, failed = without_threads do
      Kobako::Sandbox.new(wasm_path: WASM, timeout: 1)
      :built
    rescue Kobako::Error => e
      e.class
    end
    print [refused, failed, outcome(Kobako::Sandbox.new(wasm_path: WASM, timeout: 1), "loop {}")].inspect
  RUBY

  # @behavior S-254
  def test_a_first_sandbox_the_host_has_no_thread_for_raises_a_setup_error
    answer = run_exhausted_child(NO_THREAD_FOR_THE_FIRST_SANDBOX)
    assert_equal "[true, Kobako::SetupError, Kobako::TimeoutError]", answer,
                 "the first Kobako::Sandbox.new in a process whose RLIMIT_NPROC refuses Thread.new must raise " \
                 "Kobako::SetupError, and a Sandbox built after it must stop at its timeout"
  end

  NO_ROOM_FOR_AN_INSTANCE = <<~RUBY
    queue = Queue.new
    sandbox = Kobako::Sandbox.new(wasm_path: WASM, gvl: %<gvl>s).bind("Blocker", Blocker.new(queue))
    neighbour = Thread.new { sandbox.eval("Blocker.wait; :ended").value }
    Thread.pass until neighbour.status == "sleep"
    outcome(sandbox, "0")
    failed = within_address_space { outcome(sandbox, "1") }
    queue << 1
    print [failed, neighbour.value, outcome(sandbox, "2")].inspect
  RUBY

  # @behavior S-255
  def test_a_run_the_host_has_no_address_space_for_raises_a_trap
    %i[hold release].each do |gvl|
      answer = run_exhausted_child(format(NO_ROOM_FOR_AN_INSTANCE, gvl: gvl.inspect))
      assert_equal "[Kobako::TrapError, :ended, 2]", answer,
                   "Sandbox#eval (gvl: #{gvl}) on an idle guest thread in a process whose RLIMIT_AS leaves 512 MiB " \
                   "must raise Kobako::TrapError, and the waiting run and a later run must answer"
    end
  end
end
