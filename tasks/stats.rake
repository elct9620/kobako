# frozen_string_literal: true

# +rake stats+ — a rails-stats-style size report over the tracked source
# tree, one row per architectural tier. Characterization task, not part of
# the release gate (+rake default+); the helper's unit coverage rides the
# test suite (+test/tasks/test_stats.rb+).

require_relative "support/stats"

STATS_ROOT = File.expand_path("..", __dir__)

# The tier roster every size-and-churn instrument reads: +paths+ feeds
# +git ls-files+, so gitignored build products and vendored trees never
# enter the count; +kind+ places the tier — +:code+ / +:test+ weigh the
# ratio, +:code+ / +:tooling+ enter the hotspot scan, +:other+ is
# reported only. The completeness check below holds the table to the
# repo's top-level trees.
STATS_CATEGORIES = {
  "Ruby API (lib/)" => { paths: %w[lib], kind: :code },
  "Native ext (ext/)" => { paths: %w[ext], kind: :code },
  "Host crates (crates/)" => { paths: %w[crates], kind: :code },
  "Guest wasm (wasm/)" => { paths: %w[wasm], kind: :code },
  "RBS signatures (sig/)" => { paths: %w[sig], kind: :other },
  "Tests (test/)" => { paths: %w[test], kind: :test },
  "Examples (examples/)" => { paths: %w[examples], kind: :other },
  "Build tooling (tasks/ + build_config/ + bin/)" => { paths: %w[tasks build_config bin], kind: :tooling },
  "Benchmarks (benchmark/)" => { paths: %w[benchmark], kind: :tooling },
  "Docs (docs/ + SPEC.md)" => { paths: %w[docs SPEC.md], kind: :other }
}.freeze

desc "Report code statistics per architectural tier (rails-stats-style; not in release gate)."
task :stats do
  abort "cloc not on PATH; install cloc (e.g. `brew install cloc`) to run stats" unless KobakoStats.cloc_available?

  uncategorized = KobakoStats.uncategorized_dirs(
    KobakoStats.tracked_files([], root: STATS_ROOT),
    STATS_CATEGORIES.values.flat_map { |category| category[:paths] }
  )
  abort "stats: uncategorized top-level tree(s): #{uncategorized.join(", ")}" unless uncategorized.empty?

  rows = STATS_CATEGORIES.map do |name, category|
    row = KobakoStats.measure(category[:paths], root: STATS_ROOT)
    row.merge(name: name, kind: category[:kind])
  end
  puts KobakoStats.table(rows)
end
