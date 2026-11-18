# frozen_string_literal: true

require "test_helper"

# E2E — what a nested Sandbox#mount's path leads through, held through the real
# Guest Binary: each run places the mount again by that path, so no directory
# above it and no link it follows moves or goes, and no later run reaches what
# it hides.
class TestE2EMountHeld < Minitest::Test
  include E2eGuestHelper

  # Scripts changing the link +hiding_through_link+ mounts through, by name,
  # each answering +:ebusy+ when refused.
  LINK_CHANGES = {
    "delete" => 'begin; File.delete("/data/link"); :changed; rescue Errno::EBUSY; :ebusy; end',
    "rename" => 'begin; File.rename("/data/link", "/data/l2"); :changed; rescue Errno::EBUSY; :ebusy; end'
  }.freeze

  # Scripts changing the directory +hiding_past_climb+'s link climbs out of,
  # by name, each answering +:ebusy+ when refused.
  CLIMBED_CHANGES = {
    "removal" => 'begin; Dir.rmdir("/data/a"); :changed; rescue Errno::EBUSY; :ebusy; end',
    "rename" => 'begin; File.rename("/data/a", "/data/a2"); :changed; rescue Errno::EBUSY; :ebusy; end'
  }.freeze

  # A Sandbox with +outer+ at +/data+ and a read-only Tmpfs at
  # +/data/repo/hooks+, which +outer+ must hold.
  def guarding_hooks(outer)
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", outer, access: :read_write)
                   .mount("/data/repo/hooks", Kobako::Filesystem::Tmpfs.new, access: :read)
  end

  # A Sandbox with a writable Bind over +host+ at +/data+, whose
  # +repo/secret/key+ an empty Tmpfs at +/data/repo/secret+ hides.
  def hiding_secret(host)
    FileUtils.mkdir_p(File.join(host, "repo", "secret"))
    File.write(File.join(host, "repo", "secret", "key"), "hidden")
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", Kobako::Filesystem::Bind.new(host), access: :read_write)
                   .mount("/data/repo/secret", Kobako::Filesystem::Tmpfs.new, access: :read)
  end

  # A Sandbox with a writable Bind over +host+ at +/data+, whose +real/key+
  # an empty Tmpfs at +/data/link+, a link to +real+, hides.
  def hiding_through_link(host)
    Dir.mkdir(File.join(host, "real"))
    File.write(File.join(host, "real", "key"), "hidden")
    File.symlink("real", File.join(host, "link"))
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", Kobako::Filesystem::Bind.new(host), access: :read_write)
                   .mount("/data/link", Kobako::Filesystem::Tmpfs.new, access: :read)
  end

  # A Sandbox with a writable Bind over +host+ at +/data+, whose +b/key+ an
  # empty Tmpfs at +/data/l+, a link to +a/../b+, hides.
  def hiding_past_climb(host)
    FileUtils.mkdir_p([File.join(host, "a"), File.join(host, "b")])
    File.write(File.join(host, "b", "key"), "hidden")
    File.symlink("a/../b", File.join(host, "l"))
    Kobako::Sandbox.new(wasm_path: REAL_WASM)
                   .mount("/data", Kobako::Filesystem::Bind.new(host), access: :read_write)
                   .mount("/data/l", Kobako::Filesystem::Tmpfs.new, access: :read)
  end

  # @behavior MT-054
  def test_a_removal_of_a_directory_above_a_nested_mount_raises_ebusy
    sandbox = guarding_hooks(Kobako::Filesystem::Tmpfs.new("repo/hooks/.keep" => ""))
    script = 'removed = begin; Dir.rmdir("/data/repo"); :removed; rescue Errno::EBUSY; :ebusy; end; ' \
             '[removed, begin; File.write("/data/repo/hooks/x", "evil"); :wrote; rescue Errno::EROFS; :erofs; end]'

    assert_equal %i[ebusy erofs], sandbox.eval(script).value,
                 "Dir.rmdir of a directory above a read-only nested Sandbox#mount must raise EBUSY and keep it guarding"
  end

  # @behavior MT-055
  def test_a_rename_onto_a_directory_above_a_nested_mount_raises_ebusy
    sandbox = guarding_hooks(Kobako::Filesystem::Tmpfs.new("repo/hooks/.keep" => "", "q/.keep" => ""))
    script = 'File.delete("/data/q/.keep"); ' \
             'begin; File.rename("/data/q", "/data/repo"); :moved; rescue Errno::EBUSY; :ebusy; end'

    assert_equal :ebusy, sandbox.eval(script).value,
                 "File.rename onto an empty directory above a nested Sandbox#mount must raise EBUSY"
  end

  # @behavior MT-056
  def test_a_nested_mount_hides_what_its_parent_holds_in_every_later_run
    TestPaths.host_directory do |host|
      sandbox = hiding_secret(host)
      moved = sandbox.eval('begin; File.rename("/data/repo", "/data/r2"); :moved; rescue Errno::EBUSY; :ebusy; end')
      read = sandbox.eval('begin; File.read("/data/r2/secret/key"); rescue Errno::ENOENT; :enoent; end')

      assert_equal %i[ebusy enoent], [moved.value, read.value],
                   "a later Sandbox#eval must never read what a nested Sandbox#mount hides in its parent"
    end
  end

  # @behavior MT-058
  def test_a_link_a_nested_mount_path_follows_neither_moves_nor_goes
    skip "this host makes no links" if Gem.win_platform?
    LINK_CHANGES.each do |name, change|
      TestPaths.host_directory do |host|
        sandbox = hiding_through_link(host)
        changed = sandbox.eval(change)
        read = sandbox.eval('begin; File.read("/data/real/key"); rescue Errno::ENOENT; :enoent; end')

        assert_equal %i[ebusy enoent], [changed.value, read.value],
                     "a #{name} of a link a nested Sandbox#mount's path follows must raise EBUSY and keep it hiding"
      end
    end
  end

  # @behavior MT-059
  def test_a_directory_a_nested_mount_link_climbs_out_of_neither_moves_nor_goes
    skip "this host makes no links" if Gem.win_platform?
    CLIMBED_CHANGES.each do |name, change|
      TestPaths.host_directory do |host|
        sandbox = hiding_past_climb(host)
        changed = sandbox.eval(change)
        read = sandbox.eval('begin; File.read("/data/b/key"); rescue Errno::ENOENT; :enoent; end')

        assert_equal %i[ebusy enoent], [changed.value, read.value],
                     "a #{name} of a directory a Sandbox#mount's link climbs out of must raise EBUSY, still hiding"
      end
    end
  end
end
