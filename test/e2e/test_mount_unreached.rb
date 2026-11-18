# frozen_string_literal: true

require "test_helper"

# E2E — a Sandbox#mount nested inside another that Linux's mount would hide or
# refuse, through the real Guest Binary: no spelling of its path reaches it.
class TestE2EMountUnreached < Minitest::Test
  include E2eGuestHelper
  include MountReads

  # Links Linux's mount refuses to stand on, by name: each one's target and
  # the error a read beneath it raises.
  REFUSED_LINKS = { "loop" => %w[loop Errno::ELOOP], "file" => %w[f Errno::ENOTDIR],
                    "dangle" => %w[gone Errno::ENOENT] }.freeze

  # @behavior MT-047
  def test_a_mount_a_later_one_covers_is_hidden
    skip "this host makes no links" if Gem.win_platform?
    TestPaths.host_directory do |host|
      FileUtils.mkdir_p(File.join(host, "q", "t"))
      File.symlink("q/t", File.join(host, "a"))
      sandbox = bind_with(host, "/data/a" => { "note" => "A", "s/.keep" => "" }, "/data/q" => { "note" => "Q" },
                                "/data/a/s" => { "note" => "S" })

      assert_equal %w[Errno::ENOENT Errno::ENOENT Q], read_notes(sandbox, %w[/data/a /data/a/s /data/q]),
                   "a Sandbox#mount a later one covers must be hidden, and one beneath it reached nowhere, as in Linux"
    end
  end

  # @behavior MT-049
  def test_a_mount_whose_link_leads_to_nothing_or_to_no_directory_stands_nowhere
    skip "this host makes no links" if Gem.win_platform?
    REFUSED_LINKS.each do |name, (target, refusal)|
      TestPaths.host_directory do |host|
        { "note" => "outer", "f" => "" }.each { |file, body| File.write(File.join(host, file), body) }
        File.symlink(target, File.join(host, name))
        sandbox = bind_with(host, "/data/#{name}" => INNER)

        assert_equal [refusal, "outer"], read_notes(sandbox, ["/data/#{name}", "/data"]),
                     "a Sandbox#mount at a #{name} link must stand nowhere, as Linux's mount refuses it"
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
  def test_a_mount_whose_directories_above_it_stand_nowhere_stands_nowhere
    TestPaths.host_directory do |host|
      assert_equal ["Errno::ENOENT"], read_notes(bind_with(host, "/data/x/y" => INNER), %w[/data/x/y]),
                   "a Sandbox#mount beneath a directory its parent lacks must stand nowhere, as Linux refuses it"
    end
  end
end
