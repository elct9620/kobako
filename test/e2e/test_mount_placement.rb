# frozen_string_literal: true

require "test_helper"

# E2E — where a Sandbox#mount nested inside another stands, through the real
# Guest Binary, as Linux's mount answers for the same layout: over a mount on
# the same directory, and beneath its parent whatever order they were made in.
class TestE2EMountPlacement < Minitest::Test
  include E2eGuestHelper
  include MountReads

  # A Sandbox with a Bind over +host+ at +/data+ holding +real/+, a Tmpfs at
  # +/data/real+, then one holding +note+ "later" at +/data/REAL+, which a
  # host folding case answers with the same directory.
  def twice_on_real(host)
    Dir.mkdir(File.join(host, "real"))
    skip "the host directory matches names exactly" unless File.exist?(File.join(host, "REAL"))
    bind_with(host, "/data/real" => { "note" => "earlier", "x/.keep" => "" },
                    "/data/REAL" => { "note" => "later", "x/.keep" => "" })
  end

  # @behavior MT-042
  def test_two_mounts_landing_on_one_directory_show_the_later
    skip "a Bind on this host refuses a spelling its listing lacks" if Gem.win_platform?
    TestPaths.host_directory do |host|
      assert_equal %w[later later], read_notes(twice_on_real(host), %w[/data/real /data/./REAL]),
                   "of two Sandbox#mounts landing on one directory, every spelling must reach the later"
    end
  end

  # @behavior MT-043
  def test_a_mount_beneath_a_hidden_one_stands_in_the_mount_over_it
    skip "a Bind on this host refuses a spelling its listing lacks" if Gem.win_platform?
    TestPaths.host_directory do |host|
      sandbox = twice_on_real(host).mount("/data/real/x", Kobako::Filesystem::Tmpfs.new(INNER), access: :read)

      assert_equal %w[inner inner], read_notes(sandbox, %w[/data/real /data/./REAL], "x/note"),
                   "a Sandbox#mount beneath a hidden one must stand in the mount over it, by every spelling"
    end
  end

  # @behavior MT-044
  def test_a_nested_mount_made_before_its_parent_stands_beneath_it
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                             .mount("/data/a/b", Kobako::Filesystem::Tmpfs.new("note" => "inner"), access: :read)
                             .mount("/data/a", Kobako::Filesystem::Tmpfs.new("b/.keep" => ""), access: :read)
                             .mount("/data", Kobako::Filesystem::Tmpfs.new("a/.keep" => ""), access: :read)

    assert_equal ["inner"], read_notes(sandbox, %w[/data/./a/b]),
                 "a Sandbox#mount nested beneath one made after it must still be reached"
  end
end
