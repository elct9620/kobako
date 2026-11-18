# frozen_string_literal: true

require "test_helper"

# E2E — where a Sandbox#mount nested inside another stands, through the real
# Guest Binary, as Linux's mount answers for the same layout: where a link at
# its path leads, over a mount landing there, and beneath its parent whatever
# order they were made in.
class TestE2EMountPlacement < Minitest::Test
  include E2eGuestHelper
  include MountReads

  # A Sandbox with a Bind over +host+ at +/data+, whose +inner+ is a link to
  # +real/+, and a Tmpfs holding +note+ "inner" at +/data/inner+.
  def at_a_link(host)
    Dir.mkdir(File.join(host, "real"))
    File.write(File.join(host, "real", "note"), "outer")
    File.symlink("real", File.join(host, "inner"))
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", Kobako::Filesystem::Bind.new(host), access: :read)
                   .mount("/data/inner", Kobako::Filesystem::Tmpfs.new("note" => "inner"), access: :read)
  end

  # @behavior MT-041
  def test_a_mount_whose_last_name_is_a_link_stands_where_the_link_leads
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      assert_equal %w[inner inner inner], read_notes(at_a_link(host), %w[/data/inner /data/./inner /data/real]),
                   "a Sandbox#mount at a link must stand on the directory the link leads to, by every spelling"
    end
  end

  # @behavior MT-042
  def test_two_mounts_landing_on_one_directory_show_the_later
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      sandbox = at_a_link(host).mount("/data/real", Kobako::Filesystem::Tmpfs.new("note" => "later"), access: :read)

      assert_equal %w[later later later later],
                   read_notes(sandbox, %w[/data/inner /data/./inner /data/real /data/./real]),
                   "of two Sandbox#mounts standing on one directory, every spelling must reach the later"
    end
  end

  # @behavior MT-043
  def test_a_mount_beneath_a_hidden_one_stands_in_the_mount_over_it
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      sandbox = at_a_link(host).mount("/data/real", Kobako::Filesystem::Tmpfs.new("x/.keep" => ""), access: :read)
                               .mount("/data/inner/x", Kobako::Filesystem::Tmpfs.new("note" => "third"), access: :read)

      assert_equal %w[third third], read_notes(sandbox, %w[/data/inner /data/./real], "x/note"),
                   "a Sandbox#mount beneath a hidden one must stand in the mount over it, by every spelling"
    end
  end

  # @behavior MT-044
  def test_a_nested_mount_made_before_its_parent_stands_beneath_it
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                             .mount("/data/a/b", Kobako::Filesystem::Tmpfs.new("note" => "inner"), access: :read)
                             .mount("/data/a", Kobako::Filesystem::Tmpfs.new("b/.keep" => ""), access: :read)
                             .mount("/data", Kobako::Filesystem::Tmpfs.new, access: :read)

    assert_equal ["inner"], read_notes(sandbox, %w[/data/./a/b]),
                 "a Sandbox#mount nested beneath one made after it must still be reached"
  end

  # @behavior MT-045
  def test_a_mount_whose_link_leads_to_its_parents_root_stands_over_it
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      File.write(File.join(host, "note"), "outer")
      File.write(File.join(host, "f"), "")
      File.symlink(".", File.join(host, "inner"))
      sandbox = bind_with(host, "/data/inner" => INNER)

      assert_equal %w[Errno::ENOENT inner Errno::ENOENT],
                   read_notes(sandbox, %w[/data/inner /data]) + read_notes(sandbox, %w[/data], "f"),
                   "a Sandbox#mount at a link to its parent's root must hide that whole Bind, as Linux's mount does"
    end
  end

  # @behavior MT-046
  def test_a_mount_whose_link_runs_through_another_mount_stands_over_that_mount
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      Dir.mkdir(File.join(host, "m"))
      File.symlink("m/sub/..", File.join(host, "x"))
      sandbox = bind_with(host, "/data/m" => { "sub/.keep" => "" }, "/data/x" => INNER)

      assert_equal %w[Errno::ENOENT inner], read_notes(sandbox, %w[/data/x /data/m]),
                   "a Sandbox#mount whose link runs through another mount must stand over that mount, as Linux's does"
    end
  end

  # @behavior MT-048
  def test_a_mount_landing_on_a_later_ones_directory_stands_over_it
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      FileUtils.mkdir_p(File.join(host, "q", "t", "s"))
      File.symlink("q/t", File.join(host, "a"))
      File.symlink("q", File.join(host, "c"))
      sandbox = bind_with(host, "/data/a/s" => { "note" => "S" }, "/data/q/t" => { "s/.keep" => "" },
                                "/data/c/t" => { "s/note" => "M" })

      assert_equal %w[M M], read_notes(sandbox, %w[/data/a/s /data/./a/s]),
                   "a Sandbox#mount landing on a later one's directory must stand over it, by every spelling"
    end
  end
end
