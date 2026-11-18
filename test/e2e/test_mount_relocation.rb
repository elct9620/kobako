# frozen_string_literal: true

require "test_helper"

# E2E — a Sandbox#mount whose link leads nowhere it can stand, through the
# real Guest Binary: it stays on the name its path spells, and stays reached
# as later mounts cover what its link ran through.
class TestE2EMountRelocation < Minitest::Test
  include E2eGuestHelper
  include MountReads

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

  # @behavior MT-048
  def test_a_mount_moved_onto_its_name_stays_reached_when_another_covers_its_parent
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      assert_equal %w[S S], read_notes(covered_twice(host), %w[/data/a/s /data/./a/s]),
                   "a Sandbox#mount whose parent another covers after it moved must be reached by every spelling"
    end
  end

  # A Sandbox with a Bind over +host+ at +/data+ linking +a+ to +q/t+ and +c+
  # to +q+, then S at +/data/a/s+, T at +/data/q/t+, and M at +/data/c/t+.
  def covered_twice(host)
    FileUtils.mkdir_p(File.join(host, "q", "t", "s"))
    File.symlink("q/t", File.join(host, "a"))
    File.symlink("q", File.join(host, "c"))
    tmpfs = Kobako::Filesystem::Tmpfs
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", Kobako::Filesystem::Bind.new(host), access: :read)
                   .mount("/data/a/s", tmpfs.new("note" => "S"), access: :read)
                   .mount("/data/q/t", tmpfs.new("s/.keep" => ""), access: :read)
                   .mount("/data/c/t", tmpfs.new("s/note" => "M"), access: :read)
  end
end
