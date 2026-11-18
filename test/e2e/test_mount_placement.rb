# frozen_string_literal: true

require "test_helper"

# E2E — where a Sandbox#mount nested inside another stands, through the real
# Guest Binary: where a link at its path leads, over an earlier mount landing
# there, and beneath its parent whatever order they were made in. A guest
# reaches each the same through a preopen's path and a walk's.
class TestE2EMountPlacement < Minitest::Test
  include E2eGuestHelper

  # Reads +note+ beneath each of +dirs+, or the error's class.
  def read_notes(sandbox, dirs, name = "note")
    script = "#{dirs.inspect}.map { |dir| begin; File.read(dir + '/#{name}'); rescue => e; e.class.to_s; end }"
    sandbox.eval(script).value
  end

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
  def test_a_mount_whose_link_leads_nowhere_it_can_stand_stays_on_its_name
    skip "this host makes no links" if Gem.win_platform?
    { "inner" => ".", "loop" => "loop", "file" => "f" }.each do |name, target|
      TestPaths.host_directory do |host|
        sandbox = at_link_to(host, name, target)
        read = read_notes(sandbox, ["/data/#{name}", "/data/./#{name}", "/data"]) + read_notes(sandbox, ["/data"], "f")
        assert_equal ["inner", "inner", "outer", ""], read,
                     "a Sandbox#mount at a link leading to its parent's root, to itself, or to a file must stand " \
                     "on its name"
      end
    end
  end

  # A Sandbox with a Bind over +host+ at +/data+ holding +note+ and a link
  # +name+ to +target+, and a Tmpfs holding +note+ "inner" at that link.
  def at_link_to(host, name, target)
    File.write(File.join(host, "note"), "outer")
    File.write(File.join(host, "f"), "")
    File.symlink(target, File.join(host, name))
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", Kobako::Filesystem::Bind.new(host), access: :read)
                   .mount("/data/#{name}", Kobako::Filesystem::Tmpfs.new("note" => "inner"), access: :read)
  end

  # @behavior MT-046
  def test_a_mount_whose_link_runs_through_a_mount_it_would_cover_stays_on_its_name
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      assert_equal %w[inner inner], read_notes(through_a_mount(host), %w[/data/x /data/./x]),
                   "a Sandbox#mount whose link runs through a mount it would cover must be reached by every spelling"
    end
  end

  # A Sandbox with a Bind over +host+ at +/data+ whose link +x+ runs through
  # +m+, a Tmpfs holding +sub/+ at +/data/m+, then one holding +note+ at +/data/x+.
  def through_a_mount(host)
    Dir.mkdir(File.join(host, "m"))
    File.symlink("m/sub/..", File.join(host, "x"))
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", Kobako::Filesystem::Bind.new(host), access: :read)
                   .mount("/data/m", Kobako::Filesystem::Tmpfs.new("sub/.keep" => ""), access: :read)
                   .mount("/data/x", Kobako::Filesystem::Tmpfs.new("note" => "inner"), access: :read)
  end

  # @behavior MT-047
  def test_a_mount_stays_reached_when_a_later_one_covers_its_links_path
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      assert_equal %w[A A S S], read_notes(covered_later(host), %w[/data/a /data/./a /data/a/s /data/./a/s]),
                   "a Sandbox#mount whose link's path a later one covers must be reached by every spelling"
    end
  end

  # A Sandbox with a Bind over +host+ at +/data+ whose link +a+ leads to
  # +q/t+, Tmpfs A at +/data/a+, then Q at +/data/q+, then S at +/data/a/s+.
  def covered_later(host)
    FileUtils.mkdir_p(File.join(host, "q", "t"))
    File.symlink("q/t", File.join(host, "a"))
    tmpfs = Kobako::Filesystem::Tmpfs
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", Kobako::Filesystem::Bind.new(host), access: :read)
                   .mount("/data/a", tmpfs.new("note" => "A", "s/.keep" => ""), access: :read)
                   .mount("/data/q", tmpfs.new("note" => "Q"), access: :read)
                   .mount("/data/a/s", tmpfs.new("note" => "S"), access: :read)
  end
end
