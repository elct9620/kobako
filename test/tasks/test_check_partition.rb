# frozen_string_literal: true

require "test_helper"
require "open3"

# The host's checks run in CI on Guest Binaries another job built, on hosts
# where beni may stage nothing, so no task check:host reaches may stage the
# mruby archive. Read from rake's own prerequisite listing, so a
# prerequisite added anywhere beneath it counts.
class KobakoCheckPartitionTest < Minitest::Test
  def test_the_host_checks_never_reach_beni
    assert_empty reachable("check:host").grep(/\Abeni:/),
                 "rake check:host must reach no beni task through any prerequisite"
  end

  def test_the_guest_checks_stage_the_archive
    assert_includes reachable("check:guest"), "beni:build",
                    "rake check:guest must reach beni:build, which the Guest Binaries are built against"
  end

  private

  # Every task +name+ reaches through its prerequisites, itself excluded.
  def reachable(name, seen = Set.new)
    prerequisites.fetch(name, []).each do |prerequisite|
      reachable(prerequisite, seen) if seen.add?(prerequisite)
    end
    seen.to_a
  end

  # Each task's prerequisites, as `rake -P` lists them.
  def prerequisites
    @prerequisites ||= begin
      out, status = Open3.capture2("bundle", "exec", "rake", "-P", chdir: TestPaths.source)
      assert status.success?, "rake -P must list the prerequisites"
      out.lines.slice_before(/\Arake /).to_h do |task, *names|
        [task.delete_prefix("rake ").strip, names.map(&:strip)]
      end
    end
  end
end
