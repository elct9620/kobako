# frozen_string_literal: true

require "test_helper"
require "tmpdir"

require_relative "../../benchmark/support/lock"

# The run-in-progress marker the Stop-hook gate reads to defer its
# CPU-heavy work while a benchmark measures. Exercised against a tmp path
# so the suite never disturbs the real tmp/.bench.lock.
class KobakoBenchLockTest < Minitest::Test
  def setup
    @dir = Dir.mktmpdir
    @path = File.join(@dir, ".bench.lock")
  end

  def teardown
    FileUtils.remove_entry(@dir)
  end

  def test_hold_marks_the_run_for_its_duration_and_clears_it_after
    inside = nil
    Kobako::Bench::Lock.hold(@path) { inside = File.exist?(@path) }

    assert inside, "a block run through Lock.hold must see the lock present while it runs"
    refute_path_exists @path, "Lock.hold must remove the lock once the block returns"
  end

  def test_the_lock_names_the_running_process
    Kobako::Bench::Lock.hold(@path) do
      pid_line, start_line = File.read(@path).lines.map(&:strip)

      assert_equal Process.pid.to_s, pid_line,
                   "the lock's first line through Lock.hold must be this process's pid so the guard can check liveness"
      refute_empty start_line,
                   "the lock's second line must carry the ps start time so a reused pid fails the guard's check"
    end
  end

  def test_a_nested_hold_leaves_the_outer_frame_owning_the_lock
    still_locked_after_inner = nil
    Kobako::Bench::Lock.hold(@path) do
      Kobako::Bench::Lock.hold(@path) { nil }
      still_locked_after_inner = File.exist?(@path)
    end

    assert still_locked_after_inner,
           "a nested Lock.hold must be a no-op that leaves the outermost frame still holding the lock"
    refute_path_exists @path, "the outermost Lock.hold must clear the lock after the nested one returns"
  end

  def test_hold_clears_the_lock_even_when_the_block_raises
    assert_raises(RuntimeError) do
      Kobako::Bench::Lock.hold(@path) { raise "boom" }
    end

    refute_path_exists @path, "Lock.hold must remove the lock even when the measured block raises"
  end
end
