# frozen_string_literal: true

require "test_helper"

# E2E — what a script makes of the device and the kind a Filesystem written in
# Ruby's +stat+ answers, through the real Guest Binary.
class TestE2ERubyFilesystemStatAnswers < Minitest::Test
  include E2eGuestHelper

  # A stat answering nothing a Filesystem may leave out.
  Stat = Data.define(:directory, :size, :ino) do
    def directory? = directory
  end

  # A stat saying what kind of entry it reports, as +File::Stat+ does.
  KindStat = Data.define(:directory, :size, :ino, :ftype) do
    def directory? = directory
  end

  # The one file +f+, whose stat by path answers +file_stat+.
  class OneFileFilesystem
    def initialize(file_stat)
      @file_stat = file_stat
    end

    def open(_path, _flags) = raise("the object's open must not be asked")

    def stat(path)
      return Stat.new(directory: true, size: 0, ino: 1) if path.empty?
      raise Errno::ENOENT, path unless path == "f"

      @file_stat
    end
  end

  # A +File.lstat+ backend reports a host's link, whose size is the length of
  # where it points.
  # @behavior RF-039
  def test_a_link_a_stat_reports_shows_as_a_link
    filesystem = OneFileFilesystem.new(KindStat.new(directory: false, size: 14, ino: 42, ftype: "link"))
    seen = Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", filesystem).eval(<<~RUBY).value
      stat = File.lstat("/data/f")
      read = begin; File.read("/data/f"); rescue Errno::ELOOP; "loop"; end
      [stat.symlink?, stat.size, read]
    RUBY

    assert_equal [true, 0, "loop"], seen,
                 "File.lstat of what a Ruby Filesystem's stat calls a link must report a link of no size, and " \
                 "File.read of it must raise Errno::ELOOP"
  end

  def on_device(dev)
    Data.define(:directory, :size, :ino) do
      define_method(:directory?) { directory }
      define_method(:dev) { dev }
    end
  end

  def same_ino_alike?(first, second)
    mounted = [first, second].map { |stat| OneFileFilesystem.new(stat.new(directory: false, size: 5, ino: 42)) }
    Kobako::Sandbox.new(wasm_path: REAL_WASM, profile: :permissive).mount("/a", mounted[0]).mount("/b", mounted[1])
                   .eval('File.stat("/a/f").ino == File.stat("/b/f").ino').value
  end

  def opening_a_file_whose_stat_says(ftype)
    filesystem = OneFileFilesystem.new(KindStat.new(directory: false, size: 0, ino: 42, ftype: ftype))
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", filesystem)
                   .eval('begin; File.open("/data/f"); rescue Errno::EACCES; "refused"; end').value
  end

  # Without a +dev+ each Filesystem stands for a device of its own, so the same
  # +ino+ from two of them is two entries; a shared +dev+ makes it one.
  # @behavior RF-032
  def test_a_stat_naming_a_device_tells_filesystems_apart_by_it
    assert_equal [false, true, false], [same_ino_alike?(Stat, Stat), same_ino_alike?(on_device(1), on_device(1)),
                                        same_ino_alike?(on_device(1), on_device(2))],
                 "File::Stat#ino through two Ruby Filesystems whose stat answers one ino must match only where both " \
                 "answer one dev"
  end

  # A FIFO's open would wait for a peer in the host's Ruby, past any deadline,
  # so an object asked to open it fails this test rather than hanging it.
  # @behavior RF-030
  def test_a_stat_naming_a_special_file_keeps_its_open_from_reaching_ruby
    %w[fifo socket blockSpecial characterSpecial].each do |ftype|
      assert_equal "refused", opening_a_file_whose_stat_says(ftype),
                   "File.open through a Ruby Filesystem whose stat answers the ftype #{ftype} must raise " \
                   "Errno::EACCES without asking the object to open it"
    end
  end
end
