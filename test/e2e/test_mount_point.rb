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
  def inside(outer)
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", outer, access: :read_write)
                   .mount("/data/inner", Kobako::Filesystem::Tmpfs.new("note" => "inner"), access: :read_write)
  end

  # @behavior MT-036
  def test_a_mount_point_neither_moves_nor_goes
    sandbox = inside(Kobako::Filesystem::Tmpfs.new("inner/.keep" => ""))
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
    sandbox = inside(Kobako::Filesystem::Tmpfs.new("inner/.keep" => ""))
    script = '[File.rename("/data/./inner", "/data/./inner"), File.read("/data/inner/note")]'

    assert_equal [0, "inner"], sandbox.eval(script).value,
                 "File.rename of a Sandbox#mount's mount point to its own name must return 0 and keep the mount"
  end
end
