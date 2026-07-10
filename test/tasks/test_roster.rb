# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/roster"
require_relative "../../tasks/support/stats"

# Unit coverage for the tier roster shared by the size, churn, and
# pub-surface instruments: kind-based path selection and the
# completeness guard that holds the table to the repo's top-level
# trees. Fixture rosters keep the rule tests about the rule; the live
# tests then pin the real table to the real repo inside the gated
# suite.
class KobakoRosterTest < Minitest::Test
  Roster = KobakoRoster

  FIXTURE = {
    "Ruby API (lib/)" => { paths: %w[lib], kind: :code },
    "Tests (test/)" => { paths: %w[test], kind: :test },
    "Build tooling (tasks/ + bin/)" => { paths: %w[tasks bin], kind: :tooling }
  }.freeze

  def test_tier_paths_select_the_matching_kinds_in_roster_order
    assert_equal %w[lib tasks bin], Roster.tier_paths(%i[code tooling], categories: FIXTURE),
                 "a kind set through tier_paths must yield every matching tier's paths in roster order"
  end

  def test_tier_paths_of_an_unused_kind_are_empty
    assert_empty Roster.tier_paths(%i[docs], categories: FIXTURE),
                 "a kind no roster entry carries must select no paths"
  end

  # The roster's staleness half, mirroring the ledger rule of the other
  # instruments: a tier is live while any of its paths still holds a
  # tracked file — directory paths match by prefix, file paths (the
  # SPEC.md shape) exactly.
  def test_stale_categories_name_only_tiers_with_no_tracked_file
    tracked = ["lib/kobako.rb", "SPEC.md"]
    roster = { "Ruby API (lib/)" => { paths: %w[lib], kind: :code },
               "Docs (docs/ + SPEC.md)" => { paths: %w[docs SPEC.md], kind: :other },
               "Examples (examples/)" => { paths: %w[examples], kind: :other } }

    assert_equal ["Examples (examples/)"], Roster.stale_categories(tracked, categories: roster),
                 "a roster tier none of whose paths matches a tracked file must surface as stale"
  end

  # The roster's completeness guard: a new top-level source tree must
  # enter a tier before the instruments can claim the whole repo;
  # dot-directories are repo meta and root files ride their explicit
  # category entries.
  def test_uncategorized_dirs_list_only_unplaced_top_level_trees
    tracked = ["lib/kobako.rb", "scripts/new_tool.rb", ".github/ci.yml", "Rakefile"]
    roster = { "Ruby API (lib/)" => { paths: %w[lib], kind: :code } }

    assert_equal ["scripts"], Roster.uncategorized_dirs(tracked, categories: roster),
                 "a tracked top-level tree outside every category must surface as drift"
  end

  # The gate half of the roster's repo pinning: the fixture tests above
  # hold the rules, these two hold the real table to the real repo on
  # every `rake test` run — `rake stats` alone guards only when someone
  # runs it, and it needs cloc besides.
  def test_live_roster_places_every_tracked_top_level_tree
    assert_empty Roster.uncategorized_dirs(live_tracked_paths),
                 "every tracked top-level tree through uncategorized_dirs must belong to a roster tier"
  end

  def test_live_roster_holds_no_stale_tier
    assert_empty Roster.stale_categories(live_tracked_paths),
                 "every roster tier through stale_categories must still hold a tracked file"
  end

  private

  # The same corpus the stats guard reads — tracked files minus the
  # non-implementation artifacts, so a +.keep+ mount for a gitignored
  # build product (+data/+) never reads as an unplaced tree.
  def live_tracked_paths
    KobakoStats.tracked_files([], root: File.expand_path("../..", __dir__))
  end
end
