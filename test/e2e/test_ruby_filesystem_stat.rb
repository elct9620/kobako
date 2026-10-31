# frozen_string_literal: true

require "test_helper"

# E2E — what a guest reads of an entry a Filesystem written in Ruby reports.
# The real Guest Binary reads no listing, so a fixture guest answers each
# number preview1 hands it.
class TestE2ERubyFilesystemStat < Minitest::Test
  include E2eGuestHelper

  FIXTURE = TestPaths.fixture("minimal_stat_fields.wat")
  # 1,000 seconds and five nanoseconds past the epoch, as preview1 counts them.
  MODIFIED = 1_000_000_000_005

  Fields = Data.define(:errnos, :path, :open, :listed) do
    def self.from(answer) = new(errnos: answer[0, 4], path: answer[4, 5], open: answer[9, 5], listed: answer[14])
  end

  Stat = Data.define(:directory, :size, :ino) do
    def directory? = directory
  end

  # A stat saying what kind of entry it reports, as +File::Stat+ does.
  KindStat = Data.define(:directory, :size, :ino, :ftype) do
    def directory? = directory
  end

  # A stat recording all a +File::Stat+ does beyond what a Filesystem must.
  RecordingStat = Data.define(:directory, :size, :ino, :nlink, :atime, :mtime, :ctime) do
    def directory? = directory
  end

  def recording_stat(nlink:)
    time = Time.at(1000, 5, :nsec)
    RecordingStat.new(directory: false, size: 5, ino: 42, nlink: nlink, atime: time, mtime: time, ctime: time)
  end

  # The one file +f+, each stat by path answering what +file_stat+ builds and
  # each open answering +opened+.
  class OneFileFilesystem
    def initialize(file_stat, opened)
      @file_stat = file_stat
      @opened = opened
    end

    def open(_path, _flags) = @opened

    def children(_path) = ["f"]

    def stat(path)
      return Stat.new(directory: true, size: 0, ino: 1) if path.empty?
      raise Errno::ENOENT, path unless path == "f"

      @file_stat
    end
  end

  def setup
    super
    require_fixture!(FIXTURE)
  end

  def fields(filesystem, profile:)
    Fields.from(Kobako::Sandbox.new(wasm_path: FIXTURE, profile: profile).mount("/data", filesystem).eval("").value)
  end

  def bare_file = StringIO.new(+"hello")

  # A file open answers its own size, so nothing else it reports reaches its
  # path's stat, and the inode number comes from what its lookup recorded.
  # @behavior RF-028
  def test_an_open_file_keeps_the_identity_its_lookup_recorded
    seen = fields(OneFileFilesystem.new(Stat.new(directory: false, size: 5, ino: 42), bare_file), profile: :permissive)

    assert_equal [0, 0, 0, 0], seen.errnos, "each preview1 call the fixture makes must succeed"
    refute_equal 0, seen.open[0],
                 "fd_filestat_get of a file a Ruby Filesystem opened, through a permissive guest, must name an inode " \
                 "number"
    assert_equal seen.path[0], seen.open[0],
                 "fd_filestat_get and path_filestat_get of one file a Ruby Filesystem holds must answer one inode " \
                 "number"
  end

  # A +::File+ answers +stat+, so a Filesystem handing one on reports all of it.
  # @behavior RF-029
  def test_an_open_file_answering_stat_reports_its_times_and_names
    file = bare_file
    opened_stat = recording_stat(nlink: 3)
    file.define_singleton_method(:stat) { opened_stat }
    seen = fields(OneFileFilesystem.new(Stat.new(directory: false, size: 5, ino: 42), file), profile: :permissive)

    assert_equal [3, MODIFIED, MODIFIED, MODIFIED], seen.open[1, 4],
                 "fd_filestat_get of a file answering stat, through a Ruby Filesystem, must report the count of " \
                 "names and the three times it records"
  end

  # @behavior RF-029
  def test_an_open_file_without_stat_reports_one_name_and_no_time
    seen = fields(OneFileFilesystem.new(recording_stat(nlink: 3), bare_file), profile: :permissive)

    assert_equal [1, 0, 0, 0], seen.open[1, 4],
                 "fd_filestat_get of a file with no stat, through a Ruby Filesystem, must report one name and time " \
                 "zero rather than what its path's stat says"
  end

  # The hash is drawn per process, so only agreement and its absence are
  # literal: a Ruby Filesystem names a listed entry as a stat of it does.
  # @behavior RT-183
  def test_a_permissive_guest_reads_one_inode_number_in_a_stat_and_a_listing
    seen = fields(OneFileFilesystem.new(Stat.new(directory: false, size: 5, ino: 42), bare_file), profile: :permissive)

    refute_equal 0, seen.listed,
                 "fd_readdir of a Ruby Filesystem's directory through a permissive guest must name an inode number"
    assert_equal seen.path[0], seen.listed,
                 "fd_readdir and path_filestat_get of one file a Ruby Filesystem holds must answer one inode number"
  end

  # @behavior RT-183
  def test_a_hermetic_guest_reads_no_inode_number_of_a_ruby_filesystem
    seen = fields(OneFileFilesystem.new(Stat.new(directory: false, size: 5, ino: 42), bare_file), profile: :hermetic)

    assert_equal [0, 0, 0], [seen.path[0], seen.open[0], seen.listed],
                 "path_filestat_get, fd_filestat_get, and fd_readdir of a Ruby Filesystem's file through a hermetic " \
                 "guest must answer inode number 0"
  end

  # @behavior RF-031
  def test_a_stat_reports_the_times_and_names_it_records
    filesystem = -> { OneFileFilesystem.new(recording_stat(nlink: 3), bare_file) }

    assert_equal [3, MODIFIED, MODIFIED, MODIFIED], fields(filesystem.call, profile: :permissive).path[1, 4],
                 "path_filestat_get through a permissive guest must read the count of names and the times a Ruby " \
                 "Filesystem's stat records"
    assert_equal [3, 0, 0, 0], fields(filesystem.call, profile: :hermetic).path[1, 4],
                 "path_filestat_get through a hermetic guest must read the count of names a Ruby Filesystem's stat " \
                 "records, and every time at the epoch"
  end

  # @behavior RF-031
  def test_a_stat_recording_neither_reports_one_name_and_time_zero
    seen = fields(OneFileFilesystem.new(Stat.new(directory: false, size: 5, ino: 42), bare_file), profile: :permissive)

    assert_equal [1, 0, 0, 0], seen.path[1, 4],
                 "path_filestat_get of a Ruby Filesystem's stat answering no nlink and no time must read one name " \
                 "and time zero"
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

  def opening_a_file_whose_stat_says(ftype)
    filesystem = OneFileFilesystem.new(KindStat.new(directory: false, size: 0, ino: 42, ftype: ftype), nil)
    filesystem.define_singleton_method(:open) { |*| raise "the object's open must not be asked" }
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", filesystem)
                   .eval('begin; File.open("/data/f"); rescue Errno::EACCES; "refused"; end').value
  end
end
