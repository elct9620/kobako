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

  # One Sandbox creating a file through an Overlay whose lower Ruby Layer
  # sleeps while it is asked about the new name, holding the directory,
  # and one holding the GVL looking in that directory meanwhile.
  CHILD = <<~RUBY
    require "kobako"
    Stat = Data.define(:directory, :size, :ino) { def directory? = directory }
    asking = Queue.new
    asked = 0
    layer = Object.new
    layer.define_singleton_method(:open) { |path, _flags| raise Errno::ENOENT, path }
    # A create finds its name holding the directory, so the first ask is made
    # holding it.
    layer.define_singleton_method(:stat) do |path|
      next Stat.new(true, 0, 1) if path.empty?

      if (asked += 1) == 1
        asking << true
        sleep 0.2
      end
      raise Errno::ENOENT, path
    end
    overlay = Kobako::Filesystem::Overlay.new(lower: [layer], upper: Kobako::Filesystem::Tmpfs.new)
    sandbox = lambda do |gvl|
      Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: gvl).mount("/data", overlay, access: :read_write)
    end
    maker = Thread.new { sandbox.(:hold).eval('File.write("/data/new", "x"); true').value }
    asking.pop
    looker = Thread.new { sandbox.(:hold).eval('File.exist?("/data/new")').value }
    print [maker.value, looker.value].inspect
  RUBY

  # One Sandbox creating a file in a Tmpfs while its mount's block sleeps on
  # the change, holding the directory, and one holding the GVL looking in
  # that directory meanwhile.
  POLICY_CHILD = <<~RUBY
    require "kobako"
    asking = Queue.new
    tmpfs = Kobako::Filesystem::Tmpfs.new
    block = lambda do |_path, access|
      if access == :read_write
        asking << true
        sleep 0.2
      end
      true
    end
    sandbox = lambda do |gvl|
      Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: gvl).mount("/data", tmpfs, access: :read_write, &block)
    end
    maker = Thread.new { sandbox.(:hold).eval('File.write("/data/new", "x"); true').value }
    asking.pop
    looker = Thread.new { sandbox.(:hold).eval('File.exist?("/data/new")').value }
    print [maker.value, looker.value].inspect
  RUBY

  # One Sandbox holding a directory while its mount's block sleeps, one
  # waiting on that directory whom a Timeout interrupts, and a third changing
  # it once the first is done.
  INTERRUPTED_CHILD = <<~RUBY
    require "kobako"
    require "timeout"
    asking = Queue.new
    tmpfs = Kobako::Filesystem::Tmpfs.new
    sandbox = lambda do |&block|
      Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM")).mount("/data", tmpfs, access: :read_write, &block)
    end
    holder = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :release)
    holder.mount("/data", tmpfs, access: :read_write) do |_path, access|
      if access == :read_write
        asking << true
        sleep 0.5
      end
      true
    end
    first = Thread.new { holder.eval('File.write("/data/a", "1"); true').value }
    asking.pop
    waiter = Thread.new do
      Timeout.timeout(0.1) { sandbox.call.eval('File.write("/data/b", "2"); :not_interrupted').value }
    rescue Timeout::Error
      :timed_out
    end
    print [first.value, waiter.value, sandbox.call.eval('File.write("/data/c", "3"); true').value].inspect
  RUBY

  # Ruby checks for an interrupt as a thread comes back from waiting without
  # the GVL, which must not skip what released the lock.
  # @behavior MT-030
  def test_a_sandbox_interrupted_while_it_waits_on_a_directory_leaves_the_directory_to_the_next
    assert_equal "[true, :timed_out, true]",
                 run_ruby(INTERRUPTED_CHILD, env: { "KOBAKO_WASM" => REAL_WASM }, deadline: DEADLINE),
                 "a Timeout around a Sandbox#eval waiting on a directory another Thread holds must raise " \
                 "Timeout::Error, and a later Sandbox#eval must change that directory"
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
