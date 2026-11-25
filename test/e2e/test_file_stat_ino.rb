# frozen_string_literal: true

require "test_helper"

# E2E — the inode number a script reads through +File::Stat+, and what
# +File.identical?+ answers from it.
class TestE2EFileStatIno < Minitest::Test
  include E2eGuestHelper

  FIXTURE = TestPaths.fixture("minimal_stat_fields.wat")
  ENTRIES = '[File.stat("/d/f").ino, File.stat("/d/e").ino, File.stat("/d/dir").ino].map(&:to_s)'
  IDENTICAL = '[File.identical?("/d/f", "/d/./f"), File.identical?("/d/f", "/d/e"), File.identical?("/d/f", "/d/no")]'

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

  # A directory listing +a+ and +gone+, where +gone+ went before a stat after
  # the listing reaches it, as an entry another writer moves away does.
  class VanishingFilesystem
    def initialize = @asked = Hash.new(0)

    def open(_path, _flags) = StringIO.new(+"")

    def children(_path) = %w[a gone]

    def stat(path)
      @asked[path] += 1
      raise Errno::ENOENT, path if path == "gone" && @asked[path] > 1

      Stat.new(directory: path.empty?, size: 0, dev: 7, ino: { "" => 1, "a" => 2 }.fetch(path, 3))
    end
  end

  def setup
    super
    require_fixture(FIXTURE)
  end

  # A Sandbox with +filesystem+ at the root, which the fixture guest reads
  # through its one preopen.
  def sandbox(wasm_path, filesystem)
    Kobako::Sandbox.new(wasm_path: wasm_path, profile: :permissive).mount("/", filesystem)
  end

  # The inode number preview1 hands a fixture guest, and what a script reads.
  def both_readings(ino)
    filesystem = OneFileFilesystem.new(ino)
    [sandbox(FIXTURE, filesystem).eval("").value[4],
     Integer(sandbox(REAL_WASM, filesystem).eval('File.stat("/f").ino.to_s').value)]
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
  def test_a_script_reads_a_hash_on_the_weaker_profile
    hashes = tmpfs_entries(:permissive).map { |ino| Integer(ino) }

    assert_equal 3, hashes.uniq.size, "File::Stat#ino through a permissive guest must tell three entries apart"
    assert_operator hashes.min, :>, 2**16,
                    "File::Stat#ino through a permissive guest must read a hash, not a Tmpfs's own small numbers"
  end

  # Each Tmpfs takes the next device number of the process, so two runs
  # reading alike shows the number comes from neither.
  # @behavior RT-183 RT-428
  def test_a_hermetic_script_numbers_entries_in_the_order_it_meets_them
    assert_equal [%w[1 2 3], %w[1 2 3]], [tmpfs_entries(:hermetic), tmpfs_entries(:hermetic)],
                 "File::Stat#ino of three entries a hermetic script stats in turn must read 1, 2, and 3 in each run"
  end

  # @behavior FL-109
  def test_file_identical_answers_whether_two_paths_reach_one_entry
    answers = %i[hermetic permissive].map do |profile|
      filesystem = Kobako::Filesystem::Tmpfs.new("f" => "", "e" => "")
      Kobako::Sandbox.new(wasm_path: REAL_WASM, profile: profile).mount("/d", filesystem).eval(IDENTICAL).value
    end

    assert_equal [[true, false, false]] * 2, answers,
                 "File.identical? of one file by two spellings, of two files, and of a missing path must answer " \
                 "true, false, and false on either profile"
  end

  # @behavior FL-110
  def test_file_identical_matches_nothing_to_an_entry_reporting_no_inode_number
    answer = Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/m", OneFileFilesystem.new(0))
                            .eval('[File.stat("/m/f").ino, File.identical?("/m/f", "/m/f")]').value

    assert_equal [0, false], answer,
                 "File.identical? of a Ruby Filesystem's file reporting inode number 0 and itself must answer false"
  end

  # @behavior RT-429
  def test_a_hermetic_listing_reads_every_name_past_an_entry_gone_since
    children = %i[hermetic permissive].map do |profile|
      Kobako::Sandbox.new(wasm_path: REAL_WASM, profile: profile).mount("/m", VanishingFilesystem.new)
                     .eval('Dir.children("/m")').value
    end

    assert_equal [%w[a gone], %w[a gone]], children,
                 "Dir.children of a directory whose last entry went after the listing named it must read every " \
                 "listed name through a hermetic guest, as through a permissive one"
  end
end
