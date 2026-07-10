# frozen_string_literal: true

# +rake stats:hotspots+ — churn × size × fan-in over the source trees
# since the last release tag. Characterization signal, not part of the
# release gate (+rake default+); +rake stats:hotspots:test+ runs the
# scorer's own unit coverage.

require_relative "support/hotspots"

namespace :stats do
  namespace :hotspots do
    desc "Run the hotspot scorer's unit coverage."
    task :test do
      sh "bundle exec ruby tasks/support/hotspots_test.rb"
    end
  end

  desc "Report churn x size x fan-in hotspots since the last release tag (signal; not in release gate)."
  task :hotspots do
    tag = `git describe --tags --abbrev=0 --match "v*"`.strip
    abort "stats:hotspots: no v* release tag found" if tag.empty?

    churn = KobakoHotspots.churn(`git log #{tag}..HEAD --name-only --pretty=format:`)
    sizes = churn.keys.select { |path| File.exist?(path) }
                      .to_h { |path| [path, File.foreach(path).count] }
    lib_sources = FileList["lib/**/*.rb"].to_h { |path| [path, File.read(path)] }

    puts "hotspots since #{tag}:"
    puts "  file                                                 edits  lines fan-in"
    KobakoHotspots.rows(churn: churn, sizes: sizes, fan_in: KobakoHotspots.fan_in(lib_sources)).each do |row|
      path, edits, lines, fan = row
      puts format("  %<path>-52s %<edits>5d %<lines>6d %<fan>4d", path: path, edits: edits, lines: lines, fan: fan)
    end
  end
end
