# frozen_string_literal: true

# +rake stats+ — a rails-stats-style size report over the tracked source
# tree, one row per architectural tier. Characterization task, not part of
# the release gate (+rake default+); the helper's unit coverage rides the
# test suite (+test/tasks/test_stats.rb+).

require_relative "support/stats"

STATS_ROOT = File.expand_path("..", __dir__)

# The report's categories: extend by adding a row. +paths+ feeds
# +git ls-files+, so gitignored build products and vendored trees never
# enter the count; +kind+ decides the code-to-test ratio (+:code+ vs
# +:test+; +:other+ is reported but not weighed).
STATS_CATEGORIES = {
  "Ruby API (lib/)" => { paths: %w[lib], kind: :code },
  "Native ext (ext/)" => { paths: %w[ext], kind: :code },
  "Host crates (crates/)" => { paths: %w[crates], kind: :code },
  "Guest wasm (wasm/)" => { paths: %w[wasm], kind: :code },
  "RBS signatures (sig/)" => { paths: %w[sig], kind: :other },
  "Tests (test/)" => { paths: %w[test], kind: :test },
  "Examples (examples/)" => { paths: %w[examples], kind: :other },
  "Rake tasks (tasks/)" => { paths: %w[tasks], kind: :other },
  "Docs (docs/ + SPEC.md)" => { paths: %w[docs SPEC.md], kind: :other }
}.freeze

desc "Report code statistics per architectural tier (rails-stats-style; not in release gate)."
task :stats do
  abort "cloc not on PATH; install cloc (e.g. `brew install cloc`) to run stats" unless KobakoStats.cloc_available?

  rows = STATS_CATEGORIES.map do |name, category|
    row = KobakoStats.measure(category[:paths], root: STATS_ROOT)
    row.merge(name: name, kind: category[:kind])
  end
  puts KobakoStats.table(rows)
end
