# frozen_string_literal: true

# The architectural-tier roster shared by the size, churn, and
# pub-surface instruments — one table, so the instruments cannot drift
# apart on what the repo's tiers are. +paths+ feeds +git ls-files+, so
# gitignored build products and vendored trees never enter a scan;
# +kind+ places the tier — +:code+ / +:test+ weigh the stats ratio,
# +:code+ / +:tooling+ enter the hotspot scan, +:code+ carries the
# Rust crate trees the pub-surface scan reads, +:other+ is reported
# only. The table is pinned to the repo from both sides: the
# completeness guard flags an unplaced top-level tree, the staleness
# guard flags a tier the repo no longer holds.
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

  # The gem's source spans three tiers; a synthetic module groups them
  # as the single published gem, its native ext folded in. +slug+ names
  # the module's +rake stats:<slug>+ detail task.
  GEM_MODULE = { name: "kobako (gem)", slug: "gem", paths: %w[lib ext sig] }.freeze

  # Every path of the tiers whose +kind+ is in +kinds+, in roster order
  # — how an instrument names its scan roots without a private tier list.
  def tier_paths(kinds, categories: CATEGORIES)
    categories.values.select { |category| kinds.include?(category[:kind]) }
                     .flat_map { |category| category[:paths] }
  end

  # The repo's publishable modules — the gem, then one entry per Cargo
  # workspace member — as +{name:, paths:}+ rows for the size
  # instrument's per-module report. A code tier is a workspace only when
  # its own +Cargo.toml+ is tracked (+crates/+, +wasm/+), so +ext/+ (a
  # crate with no workspace root) folds into the gem instead of standing
  # as a rival module; members are read from the tracked tree, so a new
  # crate joins with no roster edit.
  def modules(tracked_paths, categories: CATEGORIES)
    members = tier_paths(%i[code], categories: categories)
              .select { |root| tracked_paths.include?("#{root}/Cargo.toml") }
              .flat_map { |root| workspace_members(root, tracked_paths) }
    [GEM_MODULE, *members]
  end

  # The direct subdirectories of +root+ that carry a +Cargo.toml+, each
  # a module named for — and reached at +rake stats:<slug>+ by — its
  # crate directory.
  def workspace_members(root, tracked_paths)
    pattern = %r{\A(#{Regexp.escape(root)}/[^/]+)/Cargo\.toml\z}
    tracked_paths.filter_map { |path| path[pattern, 1] }
                 .map { |dir| { name: File.basename(dir), slug: File.basename(dir), paths: [dir] } }
  end

  # The tracked top-level directories the roster fails to place — a
  # new source tree must enter a tier before an instrument can claim
  # the whole repo. Dot-directories are repo meta, never a tier;
  # root-level files enter only through an explicit category entry.
  def uncategorized_dirs(tracked_paths, categories: CATEGORIES)
    categorized = categories.values.flat_map { |category| category[:paths] }
    tracked_paths.filter_map { |path| path[%r{\A([^/.][^/]*)/}, 1] }.uniq - categorized
  end

  # The staleness half of the completeness guard, mirroring the
  # instruments' ledger rule: a tier none of whose paths matches a
  # tracked file names a tree the repo no longer holds — dead weight to
  # shed. Directory paths match by prefix, file paths exactly.
  def stale_categories(tracked_paths, categories: CATEGORIES)
    categories.reject do |_name, category|
      category[:paths].any? do |root|
        tracked_paths.any? { |path| path == root || path.start_with?("#{root}/") }
      end
    end.keys
  end
end
