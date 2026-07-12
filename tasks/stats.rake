# frozen_string_literal: true

# +rake stats+ — a rails-stats-style size report over the tracked source
# tree, one row per architectural tier. Characterization task, not part of
# the release gate (+rake default+); the helper's unit coverage rides the
# test suite (+test/tasks/test_stats.rb+).

require_relative "support/roster"
require_relative "support/stats"

STATS_ROOT = File.expand_path("..", __dir__)

# Every stats task needs cloc for the line classification; abort with an
# install hint rather than a bare backtrace when it is off the PATH.
def stats_require_cloc!
  abort "cloc not on PATH; install cloc (e.g. `brew install cloc`) to run stats" unless KobakoStats.cloc_available?
end

# A module split into implementation and inline test LOC for the roll-up
# and detail summary. The gem's tests live in test/ (external), so its
# test share is left unmeasured rather than counting its ext crate's few
# inline lines as the whole gem's test story.
def stats_module_split(mod)
  totals = KobakoStats.measure(mod[:paths], root: STATS_ROOT)
  test = mod[:slug] == "gem" ? nil : KobakoStats.rust_test_loc(mod[:paths], root: STATS_ROOT)
  impl = test.nil? ? totals[:code] : totals[:code] - test
  { name: mod[:name], impl: impl, test: test, comment: totals[:comment] }
end

desc "Report code statistics per architectural tier (rails-stats-style; not in release gate)."
task :stats do
  stats_require_cloc!

  tracked = KobakoStats.tracked_files([], root: STATS_ROOT)
  uncategorized = KobakoRoster.uncategorized_dirs(tracked)
  abort "stats: uncategorized top-level tree(s): #{uncategorized.join(", ")}" unless uncategorized.empty?

  stale = KobakoRoster.stale_categories(tracked)
  abort "stats: stale roster tier(s) with no tracked file: #{stale.join(", ")}" unless stale.empty?

  rows = KobakoRoster::CATEGORIES.map do |name, category|
    row = KobakoStats.measure(category[:paths], root: STATS_ROOT)
    row.merge(name: name, kind: category[:kind])
  end
  rust_test = KobakoStats.rust_test_loc(KobakoRoster.tier_paths(%i[code]), root: STATS_ROOT)
  puts KobakoStats.table(rows, rust_test_loc: rust_test)
end

namespace :stats do
  desc "Report per-module code split into impl and inline test LOC (gem + each crate; not in release gate)."
  task :all do
    stats_require_cloc!

    tracked = KobakoStats.tracked_files([], root: STATS_ROOT)
    rows = KobakoRoster.modules(tracked).map { |mod| stats_module_split(mod) }
    puts KobakoStats.module_roll_up(rows)
  end

  # One +rake stats:<slug>+ per module (run +rake stats:all+ for the
  # roster); each breaks its module down by language. Defined from the
  # tracked tree so a new crate's task appears without a roster edit,
  # left out of +rake -T+ to keep the catalog to the two headline tasks.
  KobakoRoster.modules(KobakoStats.tracked_files([], root: STATS_ROOT)).each do |mod|
    task mod[:slug] do
      stats_require_cloc!

      puts "#{mod[:name]}:"
      puts KobakoStats.grid(KobakoStats.measure_languages(mod[:paths], root: STATS_ROOT))
      split = stats_module_split(mod)
      puts KobakoStats.module_summary(impl: split[:impl], test: split[:test])
    end
  end
end
