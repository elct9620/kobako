# frozen_string_literal: true

require "test_helper"

# E2E — how +File::Stat+ and the calls built on one fail, and what they leave
# undefined. Each failure's class is what CRuby 3.4 on Linux raises.
class TestE2EFileStatRefusal < Minitest::Test
  include E2eGuestHelper

  MISSING = <<~RUBY
    [-> { File.stat("/d/none") }, -> { File.lstat("/d/none") }, -> { File.ftype("/d/none") },
     -> { File::Stat.new("/d/none") }].map do |call|
      begin
        call.call
      rescue SystemCallError => e
        e.class.to_s
      end
    end
  RUBY

  UNMOUNTED = <<~RUBY
    [:stat, :lstat, :ftype, :symlink?, :size?, :zero?, :empty?].map do |call|
      begin
        File.send(call, "/d/f")
      rescue NotImplementedError => e
        e.class.to_s
      end
    end
  RUBY

  BAD_RECEIVERS = <<~'RUBY'
    file = File.open("/d/f")
    file.close
    attempt = ->(call) { begin; call.call; rescue StandardError => e; e.class.to_s; end }
    [attempt.call(-> { file.stat }), attempt.call(-> { file.lstat }), attempt.call(-> { File.stat("a\0b") }),
     attempt.call(-> { File.stat(1) })]
  RUBY

  UNDEFINED = <<~RUBY
    stat = File.stat("/d/f")
    [[:mtime, :atime, :ctime, :birthtime, :dev, :mode, :uid, :gid, :rdev, :blksize, :blocks, :pipe?, :blockdev?,
      :readable?, :writable?, :executable?, :owned?].select { |name| stat.respond_to?(name) },
     [:identical?, :mtime, :atime, :ctime].select { |name| File.respond_to?(name) },
     Object.const_defined?(:Time), $stdout.respond_to?(:stat), stat.is_a?(Comparable)]
  RUBY

  def tmpfs_sandbox
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/d", Kobako::Filesystem::Tmpfs.new("f" => "hello"))
  end

  # @behavior FL-041
  def test_each_call_asking_for_a_report_raises_enoent_for_a_missing_file
    assert_equal %w[Errno::ENOENT] * 4, tmpfs_sandbox.eval(MISSING).value,
                 "File.stat, File.lstat, File.ftype, and File::Stat.new of a missing file must raise Errno::ENOENT"
  end

  # @behavior FL-042
  def test_a_stat_with_nothing_mounted_is_unimplemented
    assert_equal %w[NotImplementedError] * 7, Kobako::Sandbox.new(wasm_path: REAL_WASM).eval(UNMOUNTED).value,
                 "File.stat and each call built on one must raise NotImplementedError with nothing mounted"
  end

  # @behavior FL-045
  def test_a_closed_file_raises_ioerror_and_a_bad_path_raises_as_a_path_does
    assert_equal %w[IOError IOError ArgumentError TypeError], tmpfs_sandbox.eval(BAD_RECEIVERS).value,
                 "File#stat and File#lstat of a closed file must raise IOError, and File.stat of a path holding NUL " \
                 "or of no String must raise ArgumentError and TypeError"
  end

  # A hermetic guest's inode numbers are all zero, so comparing two entries
  # would call them one, and the guest has no +Time+ to give a time in.
  # @behavior FL-047
  def test_what_a_report_cannot_answer_truly_it_does_not_define
    assert_equal [[], [], false, false, false], tmpfs_sandbox.eval(UNDEFINED).value,
                 "File::Stat must define no method it could only answer wrongly, File no call built on one, and IO " \
                 "no stat that outlives a removed File"
  end
end
