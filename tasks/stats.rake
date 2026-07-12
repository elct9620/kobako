# frozen_string_literal: true

# +rake stats+ — a rails-stats-style size report over the tracked source
# tree, one row per architectural tier. Characterization task, not part of
# the release gate (+rake default+); the helper's unit coverage rides the
# test suite (+test/tasks/test_stats.rb+).

require_relative "support/roster"
require_relative "support/stats"

STATS_ROOT = File.expand_path("..", __dir__)

desc "Report code statistics per architectural tier (rails-stats-style; not in release gate)."
task :stats do
  abort "cloc not on PATH; install cloc (e.g. `brew install cloc`) to run stats" unless KobakoStats.cloc_available?

  tracked = KobakoStats.tracked_files([], root: STATS_ROOT)
  uncategorized = KobakoRoster.uncategorized_dirs(tracked)
  abort "stats: uncategorized top-level tree(s): #{uncategorized.join(", ")}" unless uncategorized.empty?

  stale = KobakoRoster.stale_categories(tracked)
  abort "stats: stale roster tier(s) with no tracked file: #{stale.join(", ")}" unless stale.empty?

  rows = KobakoRoster::CATEGORIES.map do |name, category|
    row = KobakoStats.measure(category[:paths], root: STATS_ROOT)
    row.merge(name: name, kind: category[:kind])
  end
  puts KobakoStats.table(rows)
end

namespace :stats do
  desc "Report code sizes per publishable module (the gem + each Cargo workspace member; not in release gate)."
  task :all do
    abort "cloc not on PATH; install cloc (e.g. `brew install cloc`) to run stats" unless KobakoStats.cloc_available?

    tracked = KobakoStats.tracked_files([], root: STATS_ROOT)
    rows = KobakoRoster.modules(tracked).map do |mod|
      KobakoStats.measure(mod[:paths], root: STATS_ROOT).merge(name: mod[:name])
    end
    puts KobakoStats.grid(rows)
  end
end
