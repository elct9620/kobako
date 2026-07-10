# frozen_string_literal: true

# The architectural-tier roster shared by the size, churn, and
# pub-surface instruments — one table, so the instruments cannot drift
# apart on what the repo's tiers are. +paths+ feeds +git ls-files+, so
# gitignored build products and vendored trees never enter a scan;
# +kind+ places the tier — +:code+ / +:test+ weigh the stats ratio,
# +:code+ / +:tooling+ enter the hotspot scan, +:code+ carries the
# Rust crate trees the pub-surface scan reads, +:other+ is reported
# only. The completeness guard holds the table to the repo's top-level
# trees.
module KobakoRoster
  module_function

  CATEGORIES = {
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

  # Every path of the tiers whose +kind+ is in +kinds+, in roster order
  # — how an instrument names its scan roots without a private tier list.
  def tier_paths(kinds, categories: CATEGORIES)
    categories.values.select { |category| kinds.include?(category[:kind]) }
                     .flat_map { |category| category[:paths] }
  end

  # The tracked top-level directories +categorized+ fails to place — a
  # new source tree must enter a tier before an instrument can claim
  # the whole repo. Dot-directories are repo meta, never a tier;
  # root-level files enter only through an explicit category entry.
  def uncategorized_dirs(tracked_paths, categorized)
    tracked_paths.filter_map { |path| path[%r{\A([^/.][^/]*)/}, 1] }.uniq - categorized
  end
end
