# frozen_string_literal: true

require "test_helper"

# E2E — a Filesystem a Host App writes in Ruby, mounted like the kinds
# kobako ships and reached through the real Guest Binary's File.
class TestE2ERubyFilesystem < Minitest::Test
  include E2eGuestHelper

  Stat = Data.define(:directory, :size) do
    def directory? = directory
  end

  # Files held in a Hash, flat beneath the root. It answers no change
  # method, so it takes writes to its files but nothing else.
  class HashFilesystem
    attr_reader :files

    def initialize(files)
      @files = files.transform_values(&:b)
    end

    def open(path, flags)
      raise Errno::ENOENT, path unless @files.key?(path) || flags.anybits?(File::CREAT)

      @files[path] = "".b if !@files.key?(path) || flags.anybits?(File::TRUNC)
      HashFile.new(@files, path)
    end

    def stat(path)
      return Stat.new(directory: true, size: 0) if path.empty?

      Stat.new(directory: false, size: @files.fetch(path) { raise Errno::ENOENT, path }.bytesize)
    end
  end

  # One file of a HashFilesystem, answering the IO subset a file needs.
  class HashFile
    def initialize(files, path)
      @files = files
      @path = path
    end

    def pread(length, offset) = @files.fetch(@path).byteslice(offset, length)

    def pwrite(data, offset)
      content = @files.fetch(@path).ljust(offset, "\0")
      @files[@path] = content.byteslice(0, offset) + data.b + (content.byteslice((offset + data.bytesize)..) || "".b)
      data.bytesize
    end

    def size = @files.fetch(@path).bytesize
  end

  def read_note(filesystem, script = 'File.read("/data/note.txt")', access: :read)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    yield sandbox if block_given?
    sandbox.mount("/data", filesystem, access: access).eval(script).value
  end

  def raising_on_open(error)
    Class.new(HashFilesystem) { define_method(:open) { |*| raise error } }.new("note.txt" => "hello")
  end

  # @behavior RF-001
  def test_a_ruby_object_answering_open_and_stat_mounts_as_a_filesystem
    assert_equal "hello", read_note(HashFilesystem.new("note.txt" => "hello")),
                 "a file a Ruby Filesystem holds read through Sandbox#mount must read back its content"
  end

  # @behavior RF-002
  def test_a_script_writes_through_a_ruby_filesystem
    filesystem = HashFilesystem.new({})
    read_note(filesystem, 'File.write("/data/new.txt", "made"); nil', access: :read_write)

    assert_equal "made", filesystem.files["new.txt"],
                 "a file written through a read-write Sandbox#mount of a Ruby Filesystem must land in the object"
  end

  # @behavior RF-003
  def test_a_change_a_ruby_filesystem_does_not_answer_is_refused_as_read_only
    script = 'begin; File.delete("/data/note.txt"); rescue Errno::EROFS; "refused"; end'

    assert_equal "refused", read_note(HashFilesystem.new("note.txt" => "hello"), script, access: :read_write),
                 "File.delete through a Ruby Filesystem with no unlink must raise Errno::EROFS"
  end

  # @behavior RF-004
  def test_an_errno_a_ruby_filesystem_raises_reaches_the_script
    script = 'begin; File.read("/data/note.txt"); rescue Errno::EACCES; "denied"; end'

    assert_equal "denied", read_note(raising_on_open(Errno::EACCES), script),
                 "an open whose Ruby Filesystem raises Errno::EACCES must raise Errno::EACCES in the script"
  end

  # @behavior RF-005
  def test_any_other_error_a_ruby_filesystem_raises_reaches_the_script_as_eio
    script = 'begin; File.read("/data/note.txt"); rescue Errno::EIO; "failed"; end'

    assert_equal "failed", read_note(raising_on_open(RuntimeError), script),
                 "an open whose Ruby Filesystem raises a RuntimeError must raise Errno::EIO in the script"
  end

  # @behavior RF-006
  def test_a_close_a_ruby_filesystems_file_refuses_fails_the_scripts_close
    refusing = Class.new(HashFile) { define_method(:close) { raise Errno::EACCES } }
    filesystem = HashFilesystem.new("note.txt" => "hello")
    filesystem.define_singleton_method(:open) { |path, _flags| refusing.new(files, path) }
    script = 'f = File.open("/data/note.txt"); begin; f.close; rescue Errno::EACCES; "refused"; end'

    assert_equal "refused", read_note(filesystem, script),
                 "File#close whose Ruby file raises Errno::EACCES must raise Errno::EACCES in the script"
  end

  # The file object is referred to from the guest's descriptor alone, so
  # only its registration keeps a collection at every allocation from
  # freeing it between two reads.
  # @behavior RF-007
  def test_a_file_the_script_holds_open_outlives_the_hosts_garbage_collection
    script = 'f = File.open("/data/note.txt"); a = f.read(2); Host::Churn.call; a + f.read(3)'
    GC.stress = true
    read = read_note(HashFilesystem.new("note.txt" => "hello"), script) do |sandbox|
      sandbox.bind("Host::Churn", -> { Array.new(20) { |i| "s#{i}" }.size })
    end

    assert_equal "hello", read, "a file held open through a Ruby Filesystem must read on after every collection"
  ensure
    GC.stress = false
  end

  # A HashFilesystem whose files claim ten times what they wrote.
  def overclaiming_filesystem
    overclaims = Class.new(HashFile) { define_method(:pwrite) { |data, offset| super(data, offset) * 10 } }
    filesystem = HashFilesystem.new("note.txt" => "")
    filesystem.define_singleton_method(:open) { |path, _flags| overclaims.new(files, path) }
    filesystem
  end

  # @behavior RF-012
  def test_a_write_a_ruby_filesystem_overclaims_raises_eio
    script = 'begin; File.open("/data/note.txt", "w") { |f| f.write("hi") }; rescue Errno::EIO; "refused"; end'

    assert_equal "refused", read_note(overclaiming_filesystem, script, access: :read_write),
                 "IO#write whose Ruby file answers more bytes written than it was handed must raise Errno::EIO"
  end

  # What +directory:+ a Ruby Filesystem's +rename+ is told as the script
  # renames a file beneath a mount with +block+, or without one.
  def directory_told_on_rename(&block)
    told = []
    filesystem = HashFilesystem.new("a" => "x")
    filesystem.define_singleton_method(:rename) do |from, to, directory:|
      told << directory
      files[to] = files.delete(from)
    end
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/data", filesystem, access: :read_write, &block)
                   .eval('File.rename("/data/a", "/data/b")')
    told
  end

  # Only a mount with a block asks that what moves be a file.
  # @behavior RF-010
  def test_a_ruby_filesystems_rename_is_told_what_may_move
    assert_equal [[nil], [false]], [directory_told_on_rename, directory_told_on_rename { true }],
                 "a Ruby Filesystem's rename through File.rename must be told directory: false beneath a " \
                 "Sandbox#mount with a block, and directory: nil without one"
  end

  def overlay_of_a_ruby_filesystem
    Kobako::Filesystem::Overlay.new.layer(HashFilesystem.new("note.txt" => "hello"))
  end

  # The Overlay is the only thing referring to its Ruby Layer, so only its
  # marking keeps a full collection from freeing it.
  # @behavior RF-008
  def test_a_ruby_filesystem_lives_as_long_as_the_overlay_holding_it_as_a_layer
    overlay = overlay_of_a_ruby_filesystem
    3.times { GC.start(full_mark: true, immediate_sweep: true) }

    assert_equal "hello", read_note(overlay),
                 "a Ruby Filesystem held only as a Layer of an Overlay must still answer after a full collection"
  end
end
