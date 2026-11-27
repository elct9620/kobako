# frozen_string_literal: true

require "test_helper"

# E2E — a run keeping the GVL whose guest blocks in a real syscall, which only
# Ruby code can end. The child holds a Linux write lease on a file of a Bind
# mount, so the guest's open of it blocks in the kernel until the lease holder
# lets go; the kernel tells the holder with SIGIO, and the child's trap handler
# drops the lease, which needs the GVL. The guest never comes back to its run
# meanwhile, so the run hands the GVL over and takes what Ruby delivered on its
# own, or the open waits out the kernel's lease break time.
class TestE2EGvlTurnsInSyscall < Minitest::Test
  include E2eGuestHelper
  include ChildProcess

  # A child's lifetime shorter than the 45 s the kernel waits before it breaks
  # a lease itself, so a read the lease break let through is still a jam.
  MAX_LEASED_LIFETIME = 30

  # Prints what the guest read; ARGV[0] says which thread runs it.
  CHILD = <<~RUBY
    require "kobako"
    require "tmpdir"
    require "fcntl"
    F_SETOWN = 8
    F_SETLEASE = 1024
    dir = Dir.mktmpdir
    path = File.join(dir, "note")
    File.write(path, "leased")
    holder = File.open(path, "r+")
    holder.fcntl(F_SETOWN, Process.pid)
    trap("IO") { holder.fcntl(F_SETLEASE, Fcntl::F_UNLCK) }
    holder.fcntl(F_SETLEASE, Fcntl::F_WRLCK)
    sandbox = Kobako::Sandbox.new(wasm_path: ENV.fetch("KOBAKO_WASM"), gvl: :hold, timeout: nil)
                             .mount("/d", Kobako::Filesystem::Bind.new(dir), access: :read)
    read = -> { sandbox.eval('File.read("/d/note")').value }
    print(ARGV[0] == "main" ? read.call : Thread.new(&read).value)
  RUBY

  def setup
    super
    skip "a file lease is Linux's own" unless RUBY_PLATFORM.include?("linux")
  end

  # @behavior S-244
  def test_a_run_keeping_the_gvl_lets_another_thread_run_while_its_guest_blocks_in_the_host
    assert_equal "leased", leased_read("thread"),
                 "a Sandbox#eval keeping the GVL on a Thread of its own, whose guest opens a Bind file under a " \
                 "lease only the main Thread's trap handler drops, must read the file's contents"
  end

  # @behavior S-245
  def test_a_run_keeping_the_gvl_takes_what_ruby_delivers_while_its_guest_blocks_in_the_host
    assert_equal "leased", leased_read("main"),
                 "a Sandbox#eval keeping the GVL on the main Thread, whose guest opens a Bind file under a " \
                 "lease only that Thread's trap handler drops, must read the file's contents"
  end

  private

  def leased_read(thread)
    run_ruby(CHILD, thread, deadline: MAX_LEASED_LIFETIME)
  end
end
