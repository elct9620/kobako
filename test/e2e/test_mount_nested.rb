# frozen_string_literal: true

require "test_helper"

# E2E — a Sandbox#mount nested inside another through the real Guest Binary:
# reached however its path is spelled, hiding what its parent holds there,
# and busy while mounted.
class TestE2EMountNested < Minitest::Test
  include E2eGuestHelper

  # A Sandbox with +outer+ at +/data+ and +inner+ at +/data/inner+.
  def nested(outer, inner, inner_access: :read)
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", outer, access: :read_write)
                   .mount("/data/inner", inner, access: inner_access)
  end

  # A Bind over +host+ holding its own +inner/note+, which a nested mount hides.
  def hiding(host)
    Dir.mkdir(File.join(host, "inner"))
    File.write(File.join(host, "inner", "note"), "outer")
    Kobako::Filesystem::Bind.new(host)
  end

  # The host directory holds its own +inner/note+, so every spelling reading
  # the inner note shows the nested mount hides it.
  # @behavior MT-033
  def test_a_nested_mount_is_reached_however_the_path_is_spelled
    TestPaths.host_directory do |host|
      sandbox = nested(hiding(host), Kobako::Filesystem::Tmpfs.new("note" => "inner"))
      script = '["/data/./inner/note", "/data/./inner/../inner/note"].map { |at| File.read(at) }'

      assert_equal %w[inner inner], sandbox.eval(script).value,
                   "File.read spelled with . or .. must reach a nested Sandbox#mount"
    end
  end

  # @behavior MT-033
  def test_a_nested_mount_is_reached_through_a_link_beside_it
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      File.symlink("inner/note", File.join(host, "link"))
      sandbox = nested(hiding(host), Kobako::Filesystem::Tmpfs.new("note" => "inner"))

      assert_equal "inner", sandbox.eval('File.read("/data/link")').value,
                   "File.read through a link to inner/note must reach a nested Sandbox#mount"
    end
  end

  # A host folding case answers +INNER+ with the directory the inner mount
  # stands on, so the read reaches the mount rather than what it hides.
  # @behavior MT-033
  def test_a_nested_mount_is_reached_by_a_spelling_its_folding_host_answers
    skip "a Bind on this host refuses a spelling its listing lacks" if Gem.win_platform?
    TestPaths.host_directory do |host|
      outer = hiding(host)
      skip "the host directory matches names exactly" unless File.exist?(File.join(host, "INNER"))
      sandbox = nested(outer, Kobako::Filesystem::Tmpfs.new("note" => "inner"))

      assert_equal "inner", sandbox.eval('File.read("/data/INNER/note")').value,
                   "File.read in another case on a folding host must reach the nested Sandbox#mount"
    end
  end

  # A Sandbox with a Bind over +host+ at +/data+, holding its own +a/b/note+,
  # and a Tmpfs holding +note+ at +/data/a/b+.
  def deep(host)
    FileUtils.mkdir_p(File.join(host, "a", "b"))
    File.write(File.join(host, "a", "b", "note"), "outer")
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", Kobako::Filesystem::Bind.new(host), access: :read_write)
                   .mount("/data/a/b", Kobako::Filesystem::Tmpfs.new("note" => "inner"), access: :read)
  end

  # @behavior MT-037
  def test_a_nested_mount_is_reached_through_a_link_above_it
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      sandbox = deep(host)
      File.symlink("a", File.join(host, "x"))

      assert_equal "inner", sandbox.eval('File.read("/data/./x/b/note")').value,
                   "File.read through a link to a directory above a nested Sandbox#mount must reach that mount"
    end
  end

  # @behavior MT-037
  def test_a_nested_mount_is_reached_by_a_folded_spelling_above_it
    skip "a Bind on this host refuses a spelling its listing lacks" if Gem.win_platform?
    TestPaths.host_directory do |host|
      sandbox = deep(host)
      skip "the host directory matches names exactly" unless File.exist?(File.join(host, "A"))

      assert_equal "inner", sandbox.eval('File.read("/data/./A/b/note")').value,
                   "File.read spelling a directory above a nested Sandbox#mount in another case on a folding " \
                   "host must reach that mount"
    end
  end

  # @behavior MT-038
  def test_a_nested_mount_moves_with_a_directory_above_it
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                             .mount("/data", Kobako::Filesystem::Tmpfs.new("a/b/note" => "outer"), access: :read_write)
                             .mount("/data/a/b", Kobako::Filesystem::Tmpfs.new("note" => "inner"), access: :read)
    script = 'File.rename("/data/a", "/data/c"); File.read("/data/c/b/note")'

    assert_equal "inner", sandbox.eval(script).value,
                 "File.read beneath a directory renamed above a nested Sandbox#mount must reach that mount"
  end

  # @behavior MT-034
  def test_a_nested_mount_stands_where_its_parent_holds_nothing_by_its_name
    sandbox = nested(Kobako::Filesystem::Tmpfs.new, Kobako::Filesystem::Tmpfs.new("note" => "inner"))

    assert_equal "inner", sandbox.eval('File.read("/data/./inner/note")').value,
                 "a Sandbox#mount nested where its parent holds no such name must still be reached"
  end

  # @behavior MT-035
  def test_a_read_only_nested_mount_refuses_a_change_however_the_path_is_spelled
    sandbox = nested(Kobako::Filesystem::Tmpfs.new("inner/.keep" => ""), Kobako::Filesystem::Tmpfs.new)
    script = <<~RUBY
      refused = begin; File.write("/data/./inner/new", "x"); nil; rescue Errno::EROFS; :erofs; end
      [refused, File.exist?("/data/./inner/new")]
    RUBY

    assert_equal [:erofs, false], sandbox.eval(script).value,
                 "a write beneath a read-only nested Sandbox#mount spelled with . must raise EROFS"
  end

  # @behavior MT-036
  def test_a_mount_point_neither_moves_nor_goes
    sandbox = nested(Kobako::Filesystem::Tmpfs.new("inner/.keep" => ""),
                     Kobako::Filesystem::Tmpfs.new("note" => "inner"), inner_access: :read_write)
    script = <<~RUBY
      busy = [-> { Dir.rmdir("/data/./inner") }, -> { File.rename("/data/./inner", "/data/moved") }].map do |call|
        begin; call.call; nil; rescue Errno::EBUSY; :ebusy; end
      end
      busy + [File.read("/data/./inner/note"), File.exist?("/data/moved")]
    RUBY

    assert_equal [:ebusy, :ebusy, "inner", false], sandbox.eval(script).value,
                 "removing or renaming a Sandbox#mount's mount point must raise EBUSY and change neither Filesystem"
  end
end
