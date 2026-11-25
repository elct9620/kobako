# frozen_string_literal: true

require "test_helper"

# E2E — a directory one Thread's Sandbox changes while another's looks in
# it, through one Overlay over a Filesystem written in Ruby and the real
# Guest Binary's File.
#
# The change holds the directory while it asks the Ruby Layer, which needs
# the GVL; a Thread holding the GVL and waiting for the directory would keep
# the change from ever finishing. A deadlock leaves no Thread of the process
# able to run a timeout, so the run is a child process the test kills past a
# deadline.
class TestE2EDirectoryLockThreads < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  DEADLINE = 30

  # One sandbox creating a file through an overlay whose lower Ruby layer,
  # asked about the new name holding the directory, goes on once a sandbox
  # holding the GVL waits looking in that directory.
  CHILD = <<~RUBY
    require "kobako"
    Stat = Data.define(:directory, :size, :ino) { def directory? = directory }
    asking = Queue.new
    asked = 0
    looker = nil
    layer = Object.new
    layer.define_singleton_method(:open) { |path, _flags| raise Errno::ENOENT, path }
    # A create finds its name holding the directory, so the first ask is made
    # holding it.
    layer.define_singleton_method(:stat) do |path|
      next Stat.new(true, 0, 1) if path.empty?

      if (asked += 1) == 1
        asking << true
        sleep 0.001 until looker&.stat(:runs_waiting) == 1
      end
      raise Errno::ENOENT, path
    end
    overlay = Kobako::Filesystem::Overlay.new(lower: [layer], upper: Kobako::Filesystem::Tmpfs.new)
    sandbox = lambda do |gvl|
      Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: gvl).mount("/data", overlay, access: :read_write)
    end
    maker = Thread.new { sandbox.(:hold).eval('File.write("/data/new", "x"); true').value }
    asking.pop
    looker = sandbox.(:hold)
    looking = Thread.new { looker.eval('File.exist?("/data/new")').value }
    print [maker.value, looking.value].inspect
  RUBY

  # One sandbox creating a file in a +Tmpfs+ whose mount's block, asked about
  # the change holding the directory, goes on once a sandbox holding the GVL
  # waits looking in that directory.
  POLICY_CHILD = <<~RUBY
    require "kobako"
    asking = Queue.new
    looker = nil
    tmpfs = Kobako::Filesystem::Tmpfs.new
    block = lambda do |_path, access|
      if access == :read_write
        asking << true
        sleep 0.001 until looker&.stat(:runs_waiting) == 1
      end
      true
    end
    sandbox = lambda do |gvl|
      Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: gvl).mount("/data", tmpfs, access: :read_write, &block)
    end
    maker = Thread.new { sandbox.(:hold).eval('File.write("/data/new", "x"); true').value }
    asking.pop
    looker = sandbox.(:hold)
    looking = Thread.new { looker.eval('File.exist?("/data/new")').value }
    print [maker.value, looking.value].inspect
  RUBY

  # One sandbox holding a directory in its mount's block until a second,
  # waiting on that directory, has a Timeout::Error raised into its thread,
  # as a Timeout raises; then a third changing it once the first is done.
  INTERRUPTED_CHILD = <<~RUBY
    require "kobako"
    require "timeout"
    asking = Queue.new
    release = Queue.new
    tmpfs = Kobako::Filesystem::Tmpfs.new
    holder = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :release)
    holder.mount("/data", tmpfs, access: :read_write) do |_path, access|
      if access == :read_write
        asking << true
        release.pop
      end
      true
    end
    sandbox = -> { Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM")).mount("/data", tmpfs, access: :read_write) }
    first = Thread.new { holder.eval('File.write("/data/a", "1"); true').value }
    asking.pop
    waiting = sandbox.call
    waiter = Thread.new do
      waiting.eval('File.write("/data/b", "2"); :not_interrupted').value
    rescue Timeout::Error
      :timed_out
    end
    sleep 0.001 until waiting.stat(:runs_waiting) == 1
    waiter.raise(Timeout::Error)
    interrupted = waiter.value
    release << true
    print [first.value, interrupted, sandbox.call.eval('File.write("/data/c", "3"); true').value].inspect
  RUBY

  # Ruby checks for an interrupt as a thread comes back from waiting without
  # the GVL, which must not skip what released the lock.
  # @behavior MT-030
  def test_a_sandbox_interrupted_while_it_waits_on_a_directory_leaves_the_directory_to_the_next
    assert_equal "[true, :timed_out, true]",
                 run_ruby(INTERRUPTED_CHILD, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: DEADLINE),
                 "a Timeout::Error raised into a Sandbox#eval waiting on a directory another Thread holds must " \
                 "reach the Host App, and a later Sandbox#eval must change that directory"
  end

  # A block needs the GVL to answer, so only the directory's lock giving up
  # the GVL lets it finish while the looking Sandbox waits.
  # @behavior MT-029
  def test_a_sandbox_waiting_on_a_directory_a_mounts_block_holds_lets_the_block_finish
    assert_equal "[true, true]", run_ruby(POLICY_CHILD, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: DEADLINE),
                 "a Sandbox#eval holding the GVL that looks in a directory another Thread's Sandbox#eval holds " \
                 "while its Sandbox#mount block is asked about a change must let both finish"
  end

  # The looking Sandbox waits on the directory before the Overlay is asked,
  # so only the directory's lock giving up the GVL lets the Ruby Layer
  # take the GVL back and finish.
  # @behavior MT-026
  def test_a_sandbox_waiting_on_a_directory_another_threads_change_holds_lets_the_change_finish
    assert_equal "[true, true]", run_ruby(CHILD, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: DEADLINE),
                 "a Sandbox#eval holding the GVL that looks in a directory another Thread's Sandbox#eval " \
                 "holds while creating a file over a Ruby Layer must let both finish"
  end
end
