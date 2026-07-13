# frozen_string_literal: true

require "json"

require_relative "report"

# Renders `cargo llvm-cov --json` into the concise framed table the
# coverage:crates / coverage:wasm reports print: only the files below full
# line coverage, worst first, over the workspace total. Full-coverage
# files are dropped so the report stays scannable and the paths that need
# attention — the E2E-only tiers — stand out.
module KobakoRustCoverage
  module_function

  HEADER = %w[File Lines Cover].freeze

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
