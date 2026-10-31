# frozen_string_literal: true

require "test_helper"

# E2E — the inode number a script reads through +File::Stat+.
class TestE2EFileStatIno < Minitest::Test
  include E2eGuestHelper

  FIXTURE = TestPaths.fixture("minimal_stat_fields.wat")
  ENTRIES = '[File.stat("/d/f").ino, File.stat("/d/e").ino, File.stat("/d/dir").ino].map(&:to_s)'

  Stat = Data.define(:directory, :size, :dev, :ino) do
    def directory? = directory
  end

  # The one file +f+ under inode number +ino+, on one device however many times
  # it mounts.
  class OneFileFilesystem
    def initialize(ino)
      @ino = ino
    end

    def open(_path, _flags) = StringIO.new(+"")

    def children(_path) = ["f"]

    def stat(path)
      Stat.new(directory: path.empty?, size: 0, dev: 7, ino: path.empty? ? 1 : @ino)
    end
  end

  def setup
    super
    require_fixture!(FIXTURE)
  end

  def sandbox(wasm_path, filesystem)
    Kobako::Sandbox.new(wasm_path: wasm_path, profile: :permissive).mount("/d", filesystem)
  end

  # The inode number preview1 hands a fixture guest, and what a script reads.
  def both_readings(ino)
    filesystem = OneFileFilesystem.new(ino)
    [sandbox(FIXTURE, filesystem).eval("").value[4],
     Integer(sandbox(REAL_WASM, filesystem).eval('File.stat("/d/f").ino.to_s').value)]
  end

  def readings_of_many_entries = (1..64).map { |ino| both_readings(ino) }.transpose

  def tmpfs_entries(profile)
    filesystem = Kobako::Filesystem::Tmpfs.new("f" => "", "e" => "", "dir/inner" => "")
    Kobako::Sandbox.new(wasm_path: REAL_WASM, profile: profile).mount("/d", filesystem).eval(ENTRIES).value
  end

  # A hash fills 64 bits, so an inode number saturated to mruby's 63 would
  # collapse half of them into one, and these would not agree with preview1's.
  # @behavior FL-046
  def test_an_inode_number_is_read_whole
    handed, read = readings_of_many_entries

    assert_operator handed.max, :>=, 2**63, "a 64-bit hash must reach past mruby's 63-bit Integer"
    assert_equal handed, read, "File::Stat#ino must read the number preview1 hands the guest, past 2**63 too"
  end

  # @behavior RT-183
  def test_a_script_reads_a_hash_on_the_weaker_posture_and_zero_on_the_strongest
    hashes = tmpfs_entries(:permissive).map { |ino| Integer(ino) }

    assert_equal %w[0 0 0], tmpfs_entries(:hermetic),
                 "File::Stat#ino through a hermetic guest must read 0 for every entry"
    assert_equal 3, hashes.uniq.size, "File::Stat#ino through a permissive guest must tell three entries apart"
    assert_operator hashes.min, :>, 2**16,
                    "File::Stat#ino through a permissive guest must read a hash, not a Tmpfs's own small numbers"
  end
end
