# frozen_string_literal: true

require_relative "../bench"

module KobakoBench
  # Pure release-gate judgment over two parsed results payloads: which
  # gated cases regressed past the anchor, and which gated cases one
  # payload carries that the other lacks. No IO — {Gate} owns the
  # run/anchor file handling and the abort/exit shell around these.
  #
  # The floor (+FLOOR_PCT+) is the conservative backstop SPEC.md names;
  # the noise band (+SIGMA+ combined standard deviations) can only WIDEN
  # the bar on high-variance rows, never narrow it below the floor. So
  # the gate never flags more than a bare +10% rule would — it only
  # suppresses flags on demonstrably noisy rows (the 512 KiB guest-return
  # host wrapper being the motivating false positive).
  #
  # Metric per row follows the gate policy: rows carrying +wall_time+
  # (sandbox-driven) gate on +wall_time+ — the machine-load-insensitive
  # guest budget, where a slowdown shows as a larger value; pure host
  # rows gate on the median +ips+, where a slowdown shows as a smaller
  # value. +one_shot+ / +seconds+ rows carry no dispersion and are
  # cold-path (filesystem-cache-sensitive by documentation), so they
  # are skipped.
  module Comparator
    FLOOR_PCT = 10.0
    SIGMA = 2.0

    # One gated regression that cleared the floor and the noise band.
    class Finding < Data.define(:suite, :label, :metric, :baseline, :current, :delta_pct, :band_pct)
    end

    # One gated case present in one results payload but absent from the
    # payload it is compared against. Used both directions: a case in the
    # run but not the anchor blocks the gate (a new gated case must be
    # anchored); a case in the anchor but not the run is a non-blocking
    # NOTE (a dropped benchmark the next re-bless will record).
    class MissingCase < Data.define(:suite, :label, :metric)
    end

    module_function

    # Suite names the release gate covers, derived from the roster.
    def release_suites
      RELEASE_BENCHES.map { |script| File.basename(script, ".rb") }
    end

    # Gated regressions of +current+ against +baseline+, as an Array of
    # Finding. +suites+ defaults to the release roster; cases absent from
    # the baseline are skipped here and reported by {gated_absences}.
    def compare(current, baseline, suites: release_suites)
      map_run_rows(current, baseline, suites) do |suite, label, row, base_rows|
        base = base_rows[label]
        base && finding_for(suite, label, row, base)
      end
    end

    # Gated cases present in +current+ but absent from +baseline+, as an
    # Array of MissingCase. A case is gated when it carries a gate metric
    # (+wall_time+ or +ips+); cold-path rows (+seconds+ only) are not
    # gated, so their absence is not a failure. {Gate} calls this both
    # directions: run-vs-anchor (a new case to block on) and
    # anchor-vs-run (a dropped case to NOTE).
    def gated_absences(current, baseline, suites: release_suites)
      map_run_rows(current, baseline, suites) do |suite, label, row, base_rows|
        metric = gate_metric(row)
        MissingCase.new(suite, label, metric) if metric && !base_rows.key?(label)
      end
    end

    # Walk every row of +current+ across +suites+, yielding the suite, its
    # label, the row, and +baseline+'s rows for that suite indexed by
    # label; collect each non-nil block result. The shared traversal
    # behind {compare} (a regression per row) and {gated_absences} (anchor
    # coverage per row).
    def map_run_rows(current, baseline, suites)
      suites.flat_map do |suite|
        base_rows = index(baseline.dig("suites", suite))
        index(current.dig("suites", suite)).filter_map do |label, row|
          yield(suite, label, row, base_rows)
        end
      end
    end

    def index(cases)
      (cases || []).to_h { |c| [c["label"], c] }
    end

    # Build a Finding when +row+ regressed past floor and band, else nil.
    def finding_for(suite, label, row, base)
      metric = gate_metric(row)
      return nil unless metric

      cur_c, cur_sd = central_sd(row, metric)
      base_c, base_sd = central_sd(base, metric)
      return nil if cur_c.zero? || base_c.zero?

      delta = regression_pct(metric, base_c, cur_c)
      band = noise_band(cur_c, cur_sd, base_c, base_sd)
      return nil unless delta > FLOOR_PCT && delta > band

      Finding.new(suite, label, metric, base_c, cur_c, delta, band)
    end

    # SIGMA combined relative standard deviations, as a percentage —
    # the half-width of the band a regression must clear on top of the
    # floor. Errors propagate in quadrature across the two runs.
    def noise_band(cur_c, cur_sd, base_c, base_sd)
      SIGMA * Math.sqrt((cv(cur_c, cur_sd)**2) + (cv(base_c, base_sd)**2)) * 100
    end

    # +wall_time+ when present (sandbox-driven), else +ips+, else nil
    # (one_shot / seconds rows have no dispersion to gate on).
    def gate_metric(row)
      return :wall_time if row.key?("wall_time")

      :ips if row["ips"]
    end

    def central_sd(row, metric)
      return [row["wall_time"].to_f, row["wall_time_sd"].to_f] if metric == :wall_time

      [row["ips"].to_f, row["ips_sd"].to_f]
    end

    # Regression as a positive percentage: +ips+ slows when it drops,
    # +wall_time+ slows when it rises. An improvement yields a negative
    # value, which the floor check rejects.
    def regression_pct(metric, base, cur)
      metric == :wall_time ? (cur - base) / base * 100 : (base - cur) / base * 100
    end

    def cv(central, deviation)
      central.zero? ? 0.0 : deviation / central
    end
  end
end
