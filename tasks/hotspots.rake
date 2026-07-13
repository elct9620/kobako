# frozen_string_literal: true

# +rake stats:hotspots+ — churn × size over the source trees since the
# last release tag, with require fan-in as a reference column (+-+ where
# the Ruby require scan does not reach). Characterization signal, not
# part of the release gate (+rake default+); the scorer's unit coverage
# rides the test suite (+test/tasks/test_hotspots.rb+).

require_relative "support/hotspots"
require_relative "support/roster"
require_relative "support/report"

namespace :stats do
  desc "Report churn x size x fan-in hotspots since the last release tag (signal; not in release gate)."
  task :hotspots do
    tag = `git describe --tags --abbrev=0 --match "v*"`.strip
    abort "stats:hotspots: no v* release tag found" if tag.empty?

    roots = KobakoRoster.tier_paths(%i[code tooling])
    churn = KobakoHotspots.churn(`git log #{tag}..HEAD --name-only --pretty=format:`, roots: roots)
    sizes = churn.keys.select { |path| File.exist?(path) }
                      .to_h { |path| [path, KobakoHotspots.impl_lines(path, File.read(path))] }
    ruby_sources = FileList[roots.map { |root| "#{root}/**/*.{rb,rake}" }].to_h { |path| [path, File.read(path)] }

    rows = KobakoHotspots.rows(churn: churn, sizes: sizes, fan_in: KobakoHotspots.fan_in(ruby_sources))

    puts KobakoReport.banner("stats:hotspots — churn × size × fan-in since #{tag}",
                             reads_as: "a signal, not a gate; top scored files, colder tail disclosed below")
    puts "  file                                                 edits  lines fan-in"
    rows.each do |row|
      path, edits, lines, fan = row
      puts format("  %<path>-52s %<edits>5d %<lines>6d %<fan>4s",
                  path: path, edits: edits, lines: lines, fan: fan || "-")
    end

    # Disclose the colder tail the top-N view leaves out, so a bounded
    # list never reads as the whole picture.
    total = KobakoHotspots.scored_total(churn: churn, sizes: sizes)
    puts "  ... and #{total - rows.size} more (top #{rows.size} of #{total} scored)" if total > rows.size
  end
end
