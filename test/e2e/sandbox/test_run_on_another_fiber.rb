# frozen_string_literal: true

require "test_helper"
require "weakref"

# E2E — runs on Fibers of one Thread, one suspended inside its own run's
# callback. Each run's guest stands on a thread of its own, so the runs end in
# either order, and a run whose Fiber is left suspended and collected lets go
# of what its guest held.
class TestE2ERunOnAnotherFiber < Minitest::Test
  include E2eGuestHelper

  # @behavior S-225
  def test_runs_on_two_fibers_of_one_thread_end_in_either_order
    %i[hold release].product([true, false]).each do |gvl, first_ends_first|
      assert_equal %w[a b], two_suspended_runs(gvl, first_ends_first),
                   "Sandbox#eval (gvl: #{gvl}) on two Fibers of one Thread, each suspended in its run's Service, " \
                   "must answer both runs when the #{first_ends_first ? "first" : "second"} resumes first"
    end
  end

  # @behavior S-226
  def test_a_run_whose_fiber_is_collected_while_suspended_lets_its_directory_go
    %i[hold release].each do |gvl|
      tmpfs = Kobako::Filesystem::Tmpfs.new
      abandon_suspended_run { parking_holder(tmpfs, gvl).eval('File.write("/data/a", "1")') }
      other = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 5).mount("/data", tmpfs, access: :read_write)

      assert_equal [1, true], within(2) { other.eval('File.write("/data/b", "2")').value },
                   "a Sandbox#eval writing to a Tmpfs another run (gvl: #{gvl}) held from a Fiber left suspended " \
                   "in its mount's block and collected must answer well before its deadline"
    end
  end

  # @behavior S-231
  def test_a_run_whose_fiber_is_collected_while_suspended_lets_the_objects_it_reached_go
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", Kobako::Filesystem::Tmpfs.new,
                                                              access: :read_write)
    filesystem, fiber = Thread.new { abandon_with_context_mount(sandbox) }.value
    3.times { GC.start }
    sandbox.eval("1")
    3.times { GC.start }

    assert_equal [false, false], [filesystem.weakref_alive? || false, fiber.weakref_alive? || false],
                 "a Fiber left suspended in the block of a mount its Sandbox#eval Context made, the block " \
                 "closing over the Fiber's scope, must be collected along with the mounted Tmpfs"
  end

  private

  # Weak references to the Tmpfs a Context mounted and the Fiber left suspended
  # in that mount's block, which closes over the Fiber's own scope.
  def abandon_with_context_mount(sandbox)
    tmpfs = Kobako::Filesystem::Tmpfs.new
    fiber = Fiber.new do
      sandbox.eval('File.write("/data/a", "1")') do |ctx|
        ctx.mount("/data", tmpfs, access: :read_write) { |_path, access| Fiber.yield(:parked) if access == :read_write }
      end
    end
    assert_equal :parked, fiber.resume, "the run must suspend its Fiber where the mount's block yields"
    [WeakRef.new(tmpfs), WeakRef.new(fiber)]
  end

  # The answers of two runs on Fibers of this Thread, each suspended in its
  # Service, in the order they were started.
  def two_suspended_runs(gvl, first_ends_first)
    sandbox = pausing_sandbox(gvl)
    first, second = ['Host::Pause.call("a")', 'Host::Pause.call("b")'].map do |script|
      Fiber.new { sandbox.eval(script).value }.tap(&:resume)
    end
    return [first.resume, second.resume] if first_ends_first

    second_answer = second.resume
    [first.resume, second_answer]
  end

  def pausing_sandbox(gvl)
    Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: gvl).tap do |sandbox|
      sandbox.bind("Host::Pause", lambda { |tag|
        Fiber.yield
        tag
      })
    end
  end

  # A Sandbox whose mount's block suspends its Fiber as the run writes.
  def parking_holder(tmpfs, gvl)
    Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: gvl).mount("/data", tmpfs, access: :read_write) do |_path, access|
      Fiber.yield(:parked) if access == :read_write
    end
  end

  # Leave the Fiber running +block+ suspended where it yields, then collect it.
  def abandon_suspended_run(&block)
    fiber = Fiber.new(&block)
    assert_equal :parked, fiber.resume, "the run must suspend its Fiber where the mount's block yields"
    fiber = nil # rubocop:disable Lint/UselessAssignment
    GC.start
  end

  # What the block answers, beside whether it answered within +seconds+.
  def within(seconds)
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    answer = yield
    [answer, Process.clock_gettime(Process::CLOCK_MONOTONIC) - started < seconds]
  end
end
