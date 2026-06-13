# frozen_string_literal: true

require_relative "comparator"

module KobakoBench
  # Renders a head-vs-base benchmark comparison as a Markdown report for
  # the pull-request job summary. Reuses {Comparator}'s per-row math so
  # the report flags exactly what the release gate would, but shows every
  # gated row — regression, improvement, or within-noise — rather than
  # only the regressions {Comparator.compare} returns.
  module Report
    # One compared row: the gate metric, both centrals, the signed
    # regression percentage (positive = head slower), the noise band a
    # move must clear to count, and the resulting verdict.
    class Row < Data.define(:suite, :label, :metric, :base, :head, :delta_pct, :band_pct, :status)
    end

    module_function

    # Markdown report comparing +current+ (head) against +baseline+
    # (base), both parsed results payloads from the same runner.
    def render(current, baseline, suites: Comparator.release_suites)
      rows = compare_rows(current, baseline, suites)
      notable, within_noise = rows.partition { |r| r.status != :stable }
      sections = [
        heading(current, baseline, rows),
        notable_section(notable),
        within_noise_section(within_noise),
        absence_notes(current, baseline, suites)
      ].compact
      "#{sections.join("\n\n")}\n"
    end

    # Regressions and improvements lead the report; when nothing cleared
    # the noise band, that itself is the headline.
    def notable_section(rows)
      return "✅ No gated benchmark moved beyond the noise band." if rows.empty?

      table(rows)
    end

    # The within-noise rows stay available but folded away so they do not
    # bury the moves a reviewer is looking for.
    def within_noise_section(rows)
      return nil if rows.empty?

      "<details><summary>#{rows.size} cases within noise</summary>\n\n#{table(rows)}\n</details>"
    end

    # Every gated row present in both payloads, as Row.
    def compare_rows(current, baseline, suites)
      Comparator.map_run_rows(current, baseline, suites) do |suite, label, row, base_rows|
        base = base_rows[label]
        base && row_for(suite, label, row, base)
      end
    end

    # Build a Row, or nil when the row carries no gate metric or a zero
    # central that makes a percentage meaningless.
    def row_for(suite, label, row, base)
      metric = Comparator.gate_metric(row)
      return nil unless metric

      head_c, head_sd = Comparator.central_sd(row, metric)
      base_c, base_sd = Comparator.central_sd(base, metric)
      return nil if head_c.zero? || base_c.zero?

      delta = Comparator.regression_pct(metric, base_c, head_c)
      band = Comparator.noise_band(head_c, head_sd, base_c, base_sd)
      Row.new(suite, label, metric, base_c, head_c, delta, band, status_for(delta, band))
    end

    # A regression mirrors the gate (past the floor and the noise band);
    # an improvement is a speed-up that itself clears the band; everything
    # else is movement indistinguishable from noise.
    def status_for(delta, band)
      return :regression if delta > Comparator::FLOOR_PCT && delta > band
      return :improvement if -delta > band

      :stable
    end

    STATUS_LABEL = {
      regression: "⚠️ regression",
      improvement: "🟢 improvement",
      stable: "✅ within noise"
    }.freeze

    def heading(current, baseline, rows)
      regressions = rows.count { |r| r.status == :regression }
      improvements = rows.count { |r| r.status == :improvement }
      "## Benchmark — PR head vs base (same-runner A/B)\n\n" \
        "head `#{git_sha(current)}` vs base `#{git_sha(baseline)}` · " \
        "Ruby #{current.dig("env", "ruby_version")} · " \
        "#{rows.size} gated cases · ⚠️ #{regressions} regressions · 🟢 #{improvements} improvements"
    end

    def table(rows)
      return "_No gated benchmark cases to compare._" if rows.empty?

      header = "| Suite | Case | Metric | Base | Head | Δ | Noise band | Status |\n" \
               "| --- | --- | --- | --: | --: | --: | --: | :-- |"
      body = rows.sort_by { |r| -r.delta_pct }.map { |r| row_line(r) }
      ([header] + body).join("\n")
    end

    def row_line(row)
      "| #{row.suite} | `#{row.label}` | #{row.metric} | " \
        "#{fmt(row.base, row.metric)} | #{fmt(row.head, row.metric)} | " \
        "#{format("%+.1f%%", row.delta_pct)} | #{format("±%.1f%%", row.band_pct)} | " \
        "#{STATUS_LABEL.fetch(row.status)} |"
    end

    # New gated cases (in head, not base) and dropped ones (in base, not
    # head) are reported as notes so a reviewer sees the roster shifted.
    def absence_notes(current, baseline, suites)
      notes = [
        note_line("New cases (no base to compare)", Comparator.gated_absences(current, baseline, suites: suites)),
        note_line("Dropped cases (gone from head)", Comparator.gated_absences(baseline, current, suites: suites))
      ].compact
      notes.empty? ? nil : notes.join("\n\n")
    end

    def note_line(title, cases)
      return nil if cases.empty?

      "**#{title}:** #{cases.map { |c| "`#{c.label}`" }.join(", ")}"
    end

    def fmt(value, metric)
      metric == :ips ? format("%.0f", value) : format("%.4g", value)
    end

    def git_sha(payload)
      payload.dig("env", "git_sha") || "unknown"
    end
  end
end
