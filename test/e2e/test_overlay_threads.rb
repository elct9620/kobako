# frozen_string_literal: true

require "test_helper"
require "rbconfig"

# E2E — one Overlay holding a Filesystem written in Ruby, shared by
# Sandboxes on several Threads, through the real Guest Binary's File.
#
# A Thread holding the Overlay's lock asks the Ruby Layer, which needs the
# GVL; a Thread holding the GVL and waiting for the lock would keep it from
# ever finishing. A deadlock leaves no Thread of the process able to run a
# timeout, so each run is a child process the test kills past a deadline.
class TestE2EOverlayThreads < Minitest::Test
  include E2eGuestHelper

  DEADLINE = 30

  # Two Sandboxes, one per GVL mode in ARGV, each checking a path many
  # times through one Overlay whose Ruby Layer sleeps, handing the GVL to
  # the other Thread, while it answers.
  CHILD = <<~RUBY
    require "kobako"
    Stat = Data.define(:directory, :size, :ino) { def directory? = directory }
    layer = Object.new
    def layer.open(path, _flags) = raise(Errno::ENOENT, path)
    def layer.stat(path)
      sleep 0.001
      path == "note" || path.empty? ? Stat.new(path.empty?, 0, path.empty? ? 1 : 2) : raise(Errno::ENOENT, path)
    end
    overlay = Kobako::Filesystem::Overlay.new.layer(layer)
    threads = ARGV.map do |gvl|
      Thread.new do
        sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: gvl.to_sym).mount("/data", overlay)
        sandbox.eval('(1..100).all? { File.exist?("/data/note") }').value
      end
    end
    print threads.map(&:value).inspect
  RUBY

  # What the child prints, or +:deadlocked+ when it outlives DEADLINE.
  def run_child(*modes)
    reader, writer = IO.pipe
    pid = Process.spawn({ "KOBAKO_WASM" => REAL_WASM }, RbConfig.ruby, "-I", TestPaths.source("lib"),
                        "-e", CHILD, *modes.map(&:to_s), out: writer, err: File::NULL)
    writer.close
    finished_within_deadline?(pid) ? reader.read : :deadlocked
  ensure
    reader&.close
  end

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

  # @behavior MT-019
  def test_sandboxes_holding_the_gvl_share_an_overlay_with_a_ruby_layer
    assert_equal "[true, true]", run_child(:hold, :hold),
                 "two Sandbox#eval holding the GVL on two Threads through one Overlay with a Ruby Layer " \
                 "must both finish"
  end

  # A Ruby Layer whose first +stat+ runs +nested+, recording what it answers.
  def reentering_layer(answers, &nested)
    layer = Object.new
    layer.define_singleton_method(:open) { |path, _flags| raise Errno::ENOENT, path }
    layer.define_singleton_method(:stat) do |path|
      answers << nested.call if answers.empty?
      Data.define(:directory?, :size, :ino).new(path.empty?, 0, 1)
    end
    layer
  end

  READ_X = 'begin; File.read("/data/x"); rescue Errno::EDEADLK; :deadlock; rescue Errno::ENOENT; :none; end'

  # What a script reading +/data/x+ through +overlay+ answers.
  def read_through(overlay)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", overlay).eval(READ_X).value
  end

  # The nested Sandbox runs on the Thread that already holds the Overlay's
  # lock, so waiting would never end.
  # @behavior MT-020
  def test_a_sandbox_reaching_an_overlay_from_inside_its_own_call_raises_edeadlk
    answers = []
    overlay = nil
    overlay = Kobako::Filesystem::Overlay.new.layer(reentering_layer(answers) { read_through(overlay) })
    read_through(overlay)

    assert_equal [:deadlock], answers,
                 "a Sandbox#eval reaching an Overlay from inside one of that Overlay's own Ruby Layer calls " \
                 "must raise Errno::EDEADLK in its guest"
  end

  # @behavior MT-019
  def test_sandboxes_holding_and_releasing_the_gvl_share_an_overlay_with_a_ruby_layer
    assert_equal "[true, true]", run_child(:hold, :release),
                 "a Sandbox#eval holding the GVL and one releasing it, on two Threads through one Overlay " \
                 "with a Ruby Layer, must both finish"
  end
end
