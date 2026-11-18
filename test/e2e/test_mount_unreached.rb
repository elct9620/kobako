# frozen_string_literal: true

require "test_helper"

# E2E — a Sandbox#mount nested inside another that Linux's mount would hide or
# refuse, through the real Guest Binary: no spelling reaches a hidden one, and
# one Linux refuses fails the run, as a container fails to start.
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

  # @behavior MT-054
  def test_a_nested_mount_a_past_run_moved_away_fails_the_next_run
    outer = Kobako::Filesystem::Tmpfs.new("repo/hooks/.keep" => "")
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
                             .mount("/data", outer, access: :read_write)
                             .mount("/data/repo/hooks", Kobako::Filesystem::Tmpfs.new, access: :read)
    sandbox.eval('File.rename("/data/repo", "/data/r2")')

    error = assert_raises(Kobako::SandboxError) do
      sandbox.eval('File.rename("/data/r2", "/data/repo"); File.write("/data/repo/hooks/x", "evil")')
    end
    assert_includes error.message, %("/data/repo/hooks" stands nowhere),
                    "Sandbox#eval after a run moved a read-only mount's directory away must fail before it writes"
  end
end
