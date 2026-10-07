# frozen_string_literal: true

require "json"
require "open3"

require_relative "report"
require_relative "wasm"

# Renders `cargo llvm-cov --json` into the concise framed table the
# coverage:crates / coverage:wasm reports print: only the files below full
# line coverage, worst first, over the workspace total. Full-coverage
# files are dropped so the report stays scannable and the paths that need
# attention — the E2E-only tiers — stand out.
module KobakoRustCoverage
  module_function

  HEADER = %w[File Lines Cover].freeze

  # Measure +manifest+'s whole workspace and print the report under +task+,
  # the coverage task's own name; the two Rust workspaces differ only in
  # what +scope+, +reads_as+ and +env+ say.
  def report(task, scope:, manifest:, reads_as:, env: {})
    KobakoWasm.ensure_llvm_cov!
    json, status = Open3.capture2(env, "cargo", "llvm-cov", "--manifest-path", manifest, "--workspace", "--json")
    abort "#{task}: cargo llvm-cov failed" unless status.success?

    puts KobakoReport.banner("#{task} — #{scope} line coverage, files below 100%", reads_as: reads_as)
    puts table(json, root: KobakoWasm::ROOT)
  end

  # The framed table lines for the llvm-cov export in +json_text+, with
  # absolute filenames relativized against +root+.
  def table(json_text, root:)
    export = JSON.parse(json_text)["data"].first
    KobakoReport.table(header: HEADER, rows: below_full(export["files"], root), total: total_row(export["totals"]))
  end

  # Files under full line coverage as table cells, worst-covered first.
  def below_full(files, root)
    files.map { |file| entry(file, root) }
         .select { |entry| entry[:pct] < 100 }
         .sort_by { |entry| [entry[:pct], entry[:name]] }
         .map { |entry| cells(entry) }
  end

  def entry(file, root)
    lines = file["summary"]["lines"]
    { name: file["filename"].sub("#{root}/", ""), covered: lines["covered"], count: lines["count"],
      pct: lines["percent"] }
  end

  def cells(entry)
    [entry[:name], "#{entry[:covered]}/#{entry[:count]}", "#{entry[:pct].round(1)}%"]
  end

  def total_row(totals)
    lines = totals["lines"]
    ["TOTAL", "#{lines["covered"]}/#{lines["count"]}", "#{lines["percent"].round(1)}%"]
  end
end
