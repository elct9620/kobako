# frozen_string_literal: true

require "test_helper"

# E2E — the mount point of a Sandbox#mount nested inside another, through the
# real Guest Binary: busy to a removal or a rename, after a Policy refusing it,
# and unchanged by a rename to its own name.
class TestE2EMountPoint < Minitest::Test
  include E2eGuestHelper

  # Removes and renames +/data/./inner+, answering +:ebusy+ for each change
  # refused as busy.
  BUSY = <<~RUBY
    [-> { Dir.rmdir("/data/./inner") }, -> { File.rename("/data/./inner", "/data/moved") }].map do |call|
      begin; call.call; nil; rescue Errno::EBUSY; :ebusy; end
    end
  RUBY

  # A Sandbox with +outer+ at +/data+ and a Tmpfs holding +note+ at
  # +/data/inner+, both writable.
  def nested(outer)
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", outer, access: :read_write)
                   .mount("/data/inner", Kobako::Filesystem::Tmpfs.new("note" => "inner"), access: :read_write)
  end

  # @behavior MT-034
  def test_a_nested_mount_stands_where_its_parent_holds_nothing_by_its_name
    sandbox = nested(Kobako::Filesystem::Tmpfs.new)
    script = "[File.read('/data/./inner/note')] + (#{BUSY})"

    assert_equal ["inner", :ebusy, :ebusy], sandbox.eval(script).value,
                 "a Sandbox#mount nested where its parent holds no such name must be reached, and busy to a removal " \
                 "or a rename"
  end

  # @behavior MT-036
  def test_a_mount_point_neither_moves_nor_goes
    sandbox = nested(Kobako::Filesystem::Tmpfs.new("inner/.keep" => ""))
    script = "(#{BUSY}) + [File.read('/data/./inner/note'), File.exist?('/data/moved')]"

    assert_equal [:ebusy, :ebusy, "inner", false], sandbox.eval(script).value,
                 "removing or renaming a Sandbox#mount's mount point must raise EBUSY and change neither Filesystem"
  end

  # @behavior MT-039
  def test_a_policy_refuses_a_change_at_a_mount_point_before_it_is_busy
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                             .mount("/data", Kobako::Filesystem::Tmpfs.new("inner/.keep" => ""),
                                    access: :read_write) { |path, _access| path != "/data/inner" }
                             .mount("/data/inner", Kobako::Filesystem::Tmpfs.new, access: :read_write)
    script = 'begin; Dir.rmdir("/data/./inner"); nil; rescue SystemCallError => e; e.class.to_s; end'

    assert_equal "Errno::EACCES", sandbox.eval(script).value,
                 "Dir.rmdir of a Sandbox#mount's mount point its parent's Policy refuses must raise EACCES, not EBUSY"
  end

  # @behavior MT-040
  def test_a_mount_point_renamed_to_its_own_name_stays
    sandbox = nested(Kobako::Filesystem::Tmpfs.new("inner/.keep" => ""))
    script = '[File.rename("/data/./inner", "/data/./inner"), File.read("/data/inner/note")]'

    assert_equal [0, "inner"], sandbox.eval(script).value,
                 "File.rename of a Sandbox#mount's mount point to its own name must return 0 and keep the mount"
  end

  # @behavior MT-041
  def test_a_mount_whose_last_name_is_a_link_stands_where_the_link_leads
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      Dir.mkdir(File.join(host, "real"))
      File.write(File.join(host, "real", "note"), "outer")
      File.symlink("real", File.join(host, "inner"))
      sandbox = nested(Kobako::Filesystem::Bind.new(host))

      assert_equal "inner", sandbox.eval('File.read("/data/real/note")').value,
                   "a Sandbox#mount at a link must stand on the directory the link leads to"
    end
  end

  # @behavior MT-042
  def test_two_mounts_landing_on_one_directory_show_the_later
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      Dir.mkdir(File.join(host, "real"))
      File.symlink("real", File.join(host, "inner"))
      sandbox = nested(Kobako::Filesystem::Bind.new(host))
                .mount("/data/real", Kobako::Filesystem::Tmpfs.new("note" => "later"), access: :read)

      assert_equal "later", sandbox.eval('File.read("/data/./real/note")').value,
                   "of two Sandbox#mounts standing on one directory, a read must reach the one mounted later"
    end
  end
end
