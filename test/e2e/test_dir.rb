# frozen_string_literal: true

require "test_helper"

# E2E — what a script reaches through +Dir+ beneath a Mount. Each literal is
# what CRuby 3.4 on Linux answers for the same entries.
class TestE2EDir < Minitest::Test
  include E2eGuestHelper

  # The class each call raises, or what it answers.
  ATTEMPT = "attempt = ->(call) { begin; call.call; rescue Exception => e; e.class.to_s; end }\n"

  LISTED = <<~RUBY
    each = []
    answer = Dir.each_child("/m/d") { |name| each << name }
    [Dir.children("/m/d").sort, Dir.entries("/m/d").sort, each.sort, answer]
  RUBY

  MADE_AND_REMOVED = <<~RUBY
    made = %w[/m/a /m/b /m/c].map { |path| Dir.mkdir(path) }
    seen = %w[/m/a /m/b /m/c].map { |path| Dir.exist?(path) }
    removed = [Dir.rmdir("/m/a"), Dir.delete("/m/b"), Dir.unlink("/m/c")]
    [made, seen, removed, %w[/m/a /m/b /m/c].map { |path| Dir.exist?(path) }]
  RUBY

  ASKED = <<~RUBY
    Dir.mkdir("/m/e")
    paths = %w[/m/f /m/e /m/d /m/none]
    [paths.map { |path| Dir.exist?(path) }, paths.take(3).map { |path| Dir.empty?(path) },
     attempt.(-> { Dir.empty?("/m/none") })]
  RUBY

  REFUSED = <<~RUBY
    [-> { Dir.children("/m/none") }, -> { Dir.children("/m/f") }, -> { Dir.mkdir("/m/d") },
     -> { Dir.rmdir("/m/d") }, -> { Dir.rmdir("/m/f") }].map(&attempt)
  RUBY

  MADE_READ_ONLY = "attempt.(-> { Dir.mkdir('/m/new') })"

  UNMOUNTED = <<~RUBY
    [-> { Dir.children("/m") }, -> { Dir.entries("/m") }, -> { Dir.each_child("/m") {} },
     -> { Dir.open("/m") }, -> { Dir.mkdir("/m") }, -> { Dir.rmdir("/m") },
     -> { Dir.exist?("/m") }, -> { Dir.empty?("/m") }, -> { Dir.glob("/m/*") }, -> { Dir["/m/*"] },
     -> { Dir.chdir("/m") }].map(&attempt)
  RUBY

  BAD_PERMISSION = <<~RUBY
    [-> { Dir.mkdir("/m/s", "abc") }, -> { Dir.mkdir("/m/n", nil) }].map(&attempt) +
      [Dir.exist?("/m/s"), Dir.exist?("/m/n")]
  RUBY

  def sandbox(access: :read_write)
    filesystem = Kobako::Filesystem::Tmpfs.new("f" => "hello", "d/x" => "1", "d/y" => "2")
    Kobako::Sandbox.new(wasm_path: REAL_WASM).mount("/m", filesystem, access: access)
  end

  # @behavior FL-050
  def test_a_script_lists_a_directory
    assert_equal [%w[x y], %w[. .. x y], %w[x y], nil], sandbox.eval(LISTED).value,
                 "Dir.children, Dir.entries, and Dir.each_child through the bundled guest must answer " \
                 "the directory's names, Dir.entries adding . and .."
  end

  # @behavior FL-051
  def test_a_script_makes_and_removes_directories
    assert_equal [[0, 0, 0], [true, true, true], [0, 0, 0], [false, false, false]],
                 sandbox.eval(MADE_AND_REMOVED).value,
                 "Dir.mkdir, then Dir.rmdir, Dir.delete, and Dir.unlink through the bundled guest must " \
                 "each answer 0, with Dir.exist? following each change"
  end

  # @behavior FL-052
  def test_a_script_asks_whether_a_path_is_a_directory_and_empty
    assert_equal [[false, true, true, false], [false, true, false], "Errno::ENOENT"],
                 sandbox.eval(ATTEMPT + ASKED).value,
                 "Dir.exist? and Dir.empty? through the bundled guest must answer true only for a " \
                 "directory, and for an empty one, and Dir.empty? of a missing path must raise ENOENT"
  end

  # @behavior FL-055
  def test_a_directory_call_a_path_cannot_take_raises_cruby_errno
    assert_equal %w[Errno::ENOENT Errno::ENOTDIR Errno::EEXIST Errno::ENOTEMPTY Errno::ENOTDIR],
                 sandbox.eval(ATTEMPT + REFUSED).value,
                 "a listing of a missing path or a file, a directory made twice, and a removal of a " \
                 "full directory or a file through the bundled guest must raise CRuby's Errno"
  end

  # @behavior FL-056
  def test_making_a_directory_beneath_a_read_only_mount_raises_erofs
    assert_equal "Errno::EROFS", sandbox(access: :read).eval(ATTEMPT + MADE_READ_ONLY).value,
                 "Dir.mkdir beneath a read-only Mount through the bundled guest must raise Errno::EROFS"
  end

  # @behavior FL-057
  def test_a_directory_call_with_nothing_mounted_is_unimplemented
    unmounted = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_equal ["NotImplementedError"] * 11, unmounted.eval(ATTEMPT + UNMOUNTED).value,
                 "every Dir call with nothing mounted through the bundled guest must raise NotImplementedError"
  end

  # @behavior FL-058
  def test_the_disable_file_flag_removes_dir
    flagged = Kobako::Sandbox.new(wasm_path: REAL_WASM).flag(:disable_file)

    refute flagged.eval("Object.const_defined?(:Dir)").value,
           "the disable_file flag through the bundled guest must leave no Dir constant"
  end

  # @behavior FL-071
  def test_a_permission_that_is_not_a_number_raises_type_error
    assert_equal ["TypeError", "TypeError", false, false], sandbox.eval(ATTEMPT + BAD_PERMISSION).value,
                 "Dir.mkdir with a String or nil permission through the bundled guest must raise TypeError " \
                 "and make no directory"
  end
end
