# frozen_string_literal: true

require "test_helper"

# E2E — a Sandbox#mount nested inside another that Linux's mount would hide or
# refuse, through the real Guest Binary: no spelling reaches a hidden one, one
# Linux refuses fails the run, as a container fails to start, and no run moves
# a directory above one, so no later run reaches what it hides.
class TestE2EMountUnreached < Minitest::Test
  include E2eGuestHelper
  include MountReads

  # Links Linux's mount refuses to stand on, by name: each one's target and
  # the error Linux refuses it with.
  REFUSED_LINKS = { "loop" => %w[loop ELOOP], "file" => %w[f ENOTDIR], "dangle" => %w[gone ENOENT] }.freeze

  # @behavior MT-047
  def test_a_mount_a_later_one_covers_is_hidden
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      FileUtils.mkdir_p(File.join(host, "q", "t"))
      File.symlink("q/t", File.join(host, "a"))
      sandbox = bind_with(host, "/data/a" => { "note" => "A" }, "/data/q" => { "note" => "Q" })

      assert_equal %w[Errno::ENOENT Q], read_notes(sandbox, %w[/data/a /data/q]),
                   "a Sandbox#mount a later one covers must be hidden from every spelling, as in Linux"
    end
  end

  # @behavior MT-049
  def test_a_mount_whose_link_leads_to_nothing_or_to_no_directory_fails_the_run
    skip "this host makes no links" if Gem.win_platform?
    REFUSED_LINKS.each do |name, (target, refusal)|
      TestPaths.host_directory do |host|
        File.write(File.join(host, "f"), "")
        File.symlink(target, File.join(host, name))
        error = assert_raises(Kobako::SandboxError) { bind_with(host, "/data/#{name}" => INNER).eval("nil") }
        assert_includes error.message, %("/data/#{name}" stands nowhere: #{refusal}),
                        "Sandbox#eval with a mount at a #{name} link must fail naming it, as Linux's mount refuses it"
      end
    end
  end

  # @behavior MT-050
  def test_a_mount_whose_link_leads_above_its_name_hides_that_directory
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      Dir.mkdir(File.join(host, "a"))
      File.write(File.join(host, "a", "note"), "outer")
      File.symlink(".", File.join(host, "a", "b"))

      assert_equal %w[inner Errno::ENOENT Errno::ENOENT],
                   read_notes(bind_with(host, "/data/a/b" => INNER), %w[/data/a /data/a/b /data/./a/b]),
                   "a Sandbox#mount at a link leading above its name must stand on that directory, as Linux's does"
    end
  end

  # @behavior MT-051
  def test_a_mount_whose_directories_above_it_stand_nowhere_fails_the_run
    TestPaths.host_directory do |host|
      error = assert_raises(Kobako::SandboxError) { bind_with(host, "/data/x/y" => INNER).eval("nil") }
      assert_includes error.message, %("/data/x/y" stands nowhere: ENOENT),
                      "Sandbox#eval with a mount beneath a directory its parent lacks must fail naming it"
    end
  end

  # A Sandbox with +outer+ at +/data+ and a read-only Tmpfs at
  # +/data/repo/hooks+, whose last name +outer+ does not hold.
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

  # @behavior MT-054
  def test_a_nested_mount_a_past_run_removed_the_directory_of_fails_the_next_run
    sandbox = guarding_hooks(Kobako::Filesystem::Tmpfs.new("repo/.keep" => ""))
    sandbox.eval('File.delete("/data/repo/.keep"); Dir.rmdir("/data/repo")')

    error = assert_raises(Kobako::SandboxError) do
      sandbox.eval('Dir.mkdir("/data/repo"); File.write("/data/repo/hooks/x", "evil")')
    end
    assert_includes error.message, %("/data/repo/hooks" stands nowhere),
                    "Sandbox#eval after a run removed a read-only mount's directory must fail before it writes"
  end

  # @behavior MT-055
  def test_a_rename_onto_a_directory_above_a_nested_mount_raises_ebusy
    sandbox = guarding_hooks(Kobako::Filesystem::Tmpfs.new("repo/.keep" => "", "q/.keep" => ""))
    script = 'File.delete("/data/repo/.keep", "/data/q/.keep"); ' \
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
end
