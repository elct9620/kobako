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
    # The first two asks come before the create holds the directory, and
    # sleeping in one would let the looker answer false; the third is the
    # first made holding it.
    layer.define_singleton_method(:stat) do |path|
      next Stat.new(true, 0, 1) if path.empty?

      if (asked += 1) == 3
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
