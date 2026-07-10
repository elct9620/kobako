# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/roster"

# Unit coverage for the tier roster shared by the size, churn, and
# pub-surface instruments: kind-based path selection and the
# completeness guard that holds the table to the repo's top-level
# trees. Fixture rosters keep each test about the rule, not the real
# tier set.
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

  # The roster's completeness guard: a new top-level source tree must
  # enter a tier before the instruments can claim the whole repo;
  # dot-directories are repo meta and root files ride their explicit
  # category entries.
  def test_uncategorized_dirs_list_only_unplaced_top_level_trees
    tracked = ["lib/kobako.rb", "scripts/new_tool.rb", ".github/ci.yml", "Rakefile"]

    assert_equal ["scripts"], Roster.uncategorized_dirs(tracked, ["lib"]),
                 "a tracked top-level tree outside every category must surface as drift"
  end
end
