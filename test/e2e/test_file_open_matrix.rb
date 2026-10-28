# frozen_string_literal: true

require "test_helper"
require "fileutils"
require "tmpdir"

# E2E — every way the guest opens a file, beside what CRuby on Linux
# answers for the same open. One script runs as is on both sides; the
# fixtures hold CRuby's answers over a read-write and a read-only
# directory, so each Mount must answer the same, or refuse by a rule.
class TestE2EFileOpenMatrix < Minitest::Test
  include E2eGuestHelper

  SCRIPT = "ROOT = \"/data\"\n#{File.read(TestPaths.fixture("file_open_matrix.rb"))}".freeze
  READ_WRITE = File.readlines(TestPaths.fixture("file_open_matrix_linux_rw.txt"), chomp: true).freeze
  READ_ONLY = File.readlines(TestPaths.fixture("file_open_matrix_linux_ro.txt"), chomp: true).freeze
  SEED = (0..20).flat_map do |i|
    [["f#{i}", "hello"], ["d#{i}/keep", "keep"], ["i#{i}", "hello"], ["n#{i}", "hello"]]
  end.to_h.freeze

  # Files a host directory holds, each opened as a real ::File.
  class HostFilesystem
    def initialize(root) = @root = root
    def open(path, flags) = File.open(File.join(@root, path), flags)
    def stat(path) = File.stat(File.join(@root, path))
  end

  # What the script answers beneath a Mount at /data of +filesystem+.
  def answers(filesystem, **, &block)
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", filesystem, **, &block).eval(SCRIPT).value
  end

  # A host directory holding SEED, removed when the test ends.
  def seeded_root
    root = Dir.mktmpdir
    (@roots ||= []) << root
    SEED.each do |path, content|
      FileUtils.mkdir_p(File.dirname(File.join(root, path)))
      File.write(File.join(root, path), content)
    end
    root
  end

  def teardown
    super
    @roots&.each { |root| FileUtils.remove_entry(root) }
  end

  def directory = Kobako::Filesystem::Bind.new(seeded_root)

  # @behavior FL-034 FL-035
  def test_a_read_write_directory_answers_as_cruby_does
    assert_equal READ_WRITE, answers(directory, access: :read_write),
                 "File.open and IO.new beneath a read-write Bind Mount through Sandbox#eval must answer " \
                 "as CRuby does over a read-write Linux directory"
  end

  # @behavior FL-034
  def test_a_read_write_tmpfs_answers_as_cruby_does
    assert_equal READ_WRITE, answers(Kobako::Filesystem::Tmpfs.new(SEED), access: :read_write),
                 "File.open and IO.new beneath a read-write Tmpfs Mount through Sandbox#eval must answer " \
                 "as CRuby does over a read-write Linux directory"
  end

  # @behavior FL-034
  def test_an_overlay_answers_as_cruby_does
    overlay = Kobako::Filesystem::Overlay.new(lower: [Kobako::Filesystem::Tmpfs.new(SEED)],
                                              upper: Kobako::Filesystem::Tmpfs.new)

    assert_equal READ_WRITE, answers(overlay, access: :read_write),
                 "File.open and IO.new beneath an Overlay Mount over a lower Layer through Sandbox#eval " \
                 "must answer as CRuby does over a read-write Linux directory"
  end

  # @behavior FL-034
  def test_a_read_only_directory_answers_as_cruby_does
    assert_equal READ_ONLY, answers(directory),
                 "File.open and IO.new beneath a read-only Bind Mount through Sandbox#eval must answer " \
                 "as CRuby does over a read-only Linux directory"
  end

  # @behavior FL-034
  def test_a_read_only_tmpfs_answers_as_cruby_does
    assert_equal READ_ONLY, answers(Kobako::Filesystem::Tmpfs.new(SEED)),
                 "File.open and IO.new beneath a read-only Tmpfs Mount through Sandbox#eval must answer " \
                 "as CRuby does over a read-only Linux directory"
  end

  # @behavior FL-034
  def test_a_block_allowing_every_change_answers_as_cruby_does
    assert_equal READ_WRITE, answers(directory) { |_path, _access| true },
                 "File.open beneath a Sandbox#mount block allowing every change must answer as CRuby does " \
                 "over a read-write Linux directory"
  end

  # A block refuses a change as EACCES where a read-only Mount answers
  # EROFS; every other answer is the read-only one.
  # @behavior FL-034
  def test_a_block_allowing_only_reads_refuses_each_change_as_eacces
    assert_equal READ_ONLY.map { |line| line.gsub("EROFS", "EACCES") },
                 answers(directory) { |_path, access| access == :read },
                 "File.open beneath a Sandbox#mount block allowing only reads must answer EACCES where CRuby " \
                 "over a read-only Linux directory answers EROFS"
  end

  # A Ruby Filesystem cannot append, so each write an append mode asks
  # raises ENOTSUP; everything before that write answers as CRuby does.
  # @behavior FL-034
  def test_a_ruby_filesystem_answers_as_cruby_does_but_refuses_an_append
    appends = /\A(a\+?|\S*APPEND\S*) (file|missing|IO\.new no mode): .* write=ok /
    expected = READ_WRITE.map { |line| line.match?(appends) ? line.sub(/ write=ok .*/, " write=Errno::ENOTSUP") : line }
    got = answers(HostFilesystem.new(seeded_root), access: :read_write)

    assert_equal expected, got.map { |line| line.sub(/( write=Errno::ENOTSUP) .*/, '\1') },
                 "File.open beneath a Ruby Filesystem Mount through Sandbox#eval must answer as CRuby does " \
                 "over a read-write Linux directory, but raise ENOTSUP for each append write"
  end
end
