# frozen_string_literal: true

require "test_helper"
require "tmpdir"
require "socket"

# E2E — what +File::Stat+ reads of the links and special files a Bind's host
# holds. Each literal is what CRuby 3.4 on Linux answers, unless a comment says
# why not.
class TestE2EFileStatHost < Minitest::Test
  include E2eGuestHelper

  LINKS = <<~RUBY
    link = File.lstat("/d/l")
    through = File.stat("/d/l")
    [[link.ftype, link.symlink?, link.file?], [through.ftype, through.symlink?, through.file?, through.size],
     File.ftype("/d/l"), File.lstat("/d/dangling").symlink?,
     [File.symlink?("/d/l"), File.symlink?("/d/f"), File.symlink?("/d/none"), File.symlink?("/d/dangling")]]
  RUBY

  SPECIAL = <<~RUBY
    null = File.stat("/dev/null")
    socket = File.stat("/d/sock")
    [null.chardev?, null.ftype, socket.socket?, socket.ftype, File.ftype("/d/pipe")]
  RUBY

  BENEATH_A_BLOCK = <<~RUBY
    opened = begin
      File.open("/d/secret")
    rescue Errno::EACCES => e
      e.class.to_s
    end
    [File.stat("/d/secret").size, File.lstat("/d/secret").size, File.size?("/d/secret"), opened]
  RUBY

  def setup
    super
    skip "this host has no links, FIFOs, sockets, or /dev/null" if Gem.win_platform?
  end

  def bind_sandbox(host, &block)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/d", Kobako::Filesystem::Bind.new(host), &block)
  end

  def linked(host)
    File.write(File.join(host, "f"), "hello")
    File.symlink("f", File.join(host, "l"))
    File.symlink("missing", File.join(host, "dangling"))
    bind_sandbox(host)
  end

  # @behavior FL-039
  def test_a_stat_follows_a_link_and_an_lstat_reports_it
    answer = Dir.mktmpdir { |host| linked(host).eval(LINKS).value }

    assert_equal [["link", true, false], ["file", false, true, 5], "link", true, [true, false, false, true]], answer,
                 "File.lstat, File.ftype, and File.symlink? through a Bind must read a link as itself, and File.stat " \
                 "must read what it points at"
  end

  # CRuby reads a FIFO as +"fifo"+ and +pipe?+. Preview1 has no code for one,
  # so the guest reads it as what it cannot tell, and defines no +pipe?+.
  # @behavior FL-048
  def test_a_special_file_reads_as_preview1_names_it
    answer = Dir.mktmpdir do |host|
      File.mkfifo(File.join(host, "pipe"))
      UNIXServer.open(File.join(host, "sock")) do
        bind_sandbox(host).mount("/dev", Kobako::Filesystem::Bind.new("/dev")).eval(SPECIAL).value
      end
    end

    assert_equal [true, "characterSpecial", true, "socket", "unknown"], answer,
                 "File.stat through a Bind must read a device and a socket as themselves, and a FIFO as unknown"
  end

  # A block decides each open and each change, and leaves a stat alone, so a
  # report shows no more of a refused path than +File.size+ already does.
  # @behavior FL-049
  def test_a_block_leaves_a_stat_alone_and_still_refuses_the_open
    answer = Dir.mktmpdir do |host|
      File.write(File.join(host, "secret"), "x")
      bind_sandbox(host) { |path, _access| path != "/d/secret" }.eval(BENEATH_A_BLOCK).value
    end

    assert_equal [1, 1, 1, "Errno::EACCES"], answer,
                 "File.stat and File.lstat beneath a Sandbox#mount block refusing the path must still report it, and " \
                 "File.open of it must raise Errno::EACCES"
  end
end
