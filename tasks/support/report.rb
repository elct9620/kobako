# frozen_string_literal: true

# Shared output template for the static-analysis task family. Each report
# routes its presentation through one of a fixed set of output types so
# the family reads alike and — the point the family kept losing — every
# report states its own scope in a place the type fixes:
#
#   * +table+   — a framed, aligned rails-stats grid (Signal body).
#   * +list+    — headings over indented items (Signal body).
#   * +banner+  — the top-of-report self-description for a table/list Signal.
#   * +gate+    — the verdict sign-off shared by every release-gate check:
#                 +<name>: OK — summary+ when clean, an aborting
#                 +<name>: N problem(s)+ otherwise.
#
# The measurement/rendering split each reader already follows (the
# +KobakoStats+ pair is the worked example) feeds structured rows in; this
# owns how they reach a human.
module KobakoReport
  module_function

  # A framed, aligned grid: header and optional total ruled off from the
  # body, first column left-justified and the rest right-justified.
  def table(header:, rows:, total: nil)
    widths = column_widths([header, *rows, total].compact)
    rule = table_rule(widths)
    total_lines = total ? [rule, table_row(total, widths)] : []
    [rule, table_row(header, widths), rule, *rows.map { |row| table_row(row, widths) }, *total_lines, rule]
  end

  # Headings over their indented items, flattened into one line stream.
  def list(groups)
    groups.flat_map { |heading, items| [heading, *items] }
  end

  # The top-of-report banner: the report name ruled off, with the
  # +reads_as+ scope line beneath it so the figures below are read right.
  def banner(name, reads_as: nil)
    scope = reads_as ? [reads_as] : []
    rule = "=" * [name.length, *scope.map(&:length)].max
    ["", rule, name, *scope, rule]
  end

  # The shared release-gate verdict: the +OK+ line when +violations+ is
  # empty, otherwise an aborting count named by +noun+ with the offending
  # lines beneath it.
  def gate(name:, ok_summary:, violations: [], noun: "problem")
    return "#{name}: OK — #{ok_summary}" if violations.empty?

    abort "#{name}: #{violations.size} #{noun}(s)\n#{violations.join("\n")}"
  end

  def column_widths(rows)
    rows.transpose.map { |column| column.map(&:length).max }
  end

  def table_rule(widths)
    "+#{widths.map { |width| "-" * (width + 2) }.join("+")}+"
  end

  def table_row(cells, widths)
    padded = cells.each_with_index.map do |cell, index|
      index.zero? ? cell.ljust(widths[index]) : cell.rjust(widths[index])
    end
    "| #{padded.join(" | ")} |"
  end
end
