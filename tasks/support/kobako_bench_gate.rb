# frozen_string_literal: true

require "fileutils"
require "json"

require_relative "kobako_bench"

module KobakoBench
  # Noise-aware release-gate comparator. Diffs a benchmark run against
  # the committed anchor baseline (+benchmark/baseline.json+) and reports
  # two kinds of issue: gated cases whose cumulative regression past the
  # anchor clears BOTH a relative floor and the measured noise band, and
  # gated cases the anchor does not yet cover. The anchor is fixed, not
  # the previous run, so sub-threshold drift accumulates against it
  # instead of resetting each release; it advances only by {bless!}. See
  # the Regression benchmarks section of SPEC.md.
  #
  # The floor (+FLOOR_PCT+) is the conservative backstop SPEC.md names;
  # the noise band (+SIGMA+ combined standard deviations)
  # can only WIDEN the bar on high-variance rows, never narrow it below
  # the floor. So the gate never flags more than a bare +10% rule would
  # — it only suppresses flags on demonstrably noisy rows (the 512 KiB
  # guest-return host wrapper being the motivating false positive).
  #
  # Metric per row follows the gate policy: rows carrying +wall_time+
  # (sandbox-driven) gate on +wall_time+ — the machine-load-insensitive
  # guest budget, where a slowdown shows as a larger value; pure host
  # rows gate on the median +ips+, where a slowdown shows as a smaller
  # value. +one_shot+ / +seconds+ rows carry no dispersion and are
  # cold-path (filesystem-cache-sensitive by documentation), so they
  # are skipped.
  module Gate
    FLOOR_PCT = 10.0
    SIGMA = 2.0

    # One gated regression that cleared the floor and the noise band.
    class Finding < Data.define(:suite, :label, :metric, :baseline, :current, :delta_pct, :band_pct)
    end

    # One gated case present in the run but missing from the anchor. The
    # anchor must carry every gated case, so this fails the gate until a
    # re-bless records the case in the anchor.
    class Unanchored < Data.define(:suite, :label, :metric)
    end

    RESULTS_GLOB = File.expand_path("../../benchmark/results/*.json", __dir__)
    ANCHOR_PATH = File.expand_path("../../benchmark/baseline.json", __dir__)

    module_function

    # Resolve the run and the anchor, run the comparison, print the
    # outcome, and abort (non-zero exit) on any gated regression or
    # unanchored case so the release pipeline fails. The IO/exit shell
    # around the pure {compare} / {unanchored}; the rake task delegates
    # here so the .rake stays DSL.
    def gate!(current = nil, baseline = nil)
      current, baseline = resolve(current, baseline)
      raise "bench:gate needs a current run and the committed anchor baseline" unless current && baseline

      run = load_payload(current)
      anchor = load_payload(baseline)
      regressions = compare(run, anchor)
      missing = unanchored(run, anchor)
      puts "gate: #{File.basename(current)} vs anchor #{File.basename(baseline)}"
      report(regressions, missing)
      problems = regressions.size + missing.size
      abort "gate: #{problems} gated issue(s) need review or a re-bless before release." unless problems.zero?
    end

    # Re-bless the anchor baseline from +run+ (a results JSON path),
    # replacing +benchmark/baseline.json+. This is the only way the
    # anchor moves; the cumulative budget then resets to the blessed
    # numbers, so the accepted shift and its justification must be
    # recorded in the benchmark README's "What changed" section.
    def bless!(run)
      raise "bench:bless needs a results JSON to bless as the anchor" unless run
      raise "bench:bless: #{run} does not exist" unless File.exist?(run)

      FileUtils.cp(run, ANCHOR_PATH)
      puts "blessed anchor: #{File.basename(run)} -> #{File.basename(ANCHOR_PATH)}"
      puts "record the accepted shift and why in benchmark/README.md \"What changed\" before committing."
    end

    # Resolve [current, anchor]: +current+ defaults to the newest run
    # under benchmark/results/, +baseline+ to the committed anchor
    # (benchmark/baseline.json). Either may be given explicitly.
    def resolve(current, baseline)
      current ||= Dir[RESULTS_GLOB].max_by { |path| File.mtime(path) }
      [current, baseline || ANCHOR_PATH]
    end

    def load_payload(path)
      JSON.parse(File.read(path))
    end

    # Print each unanchored case and gated regression, or a clean-pass
    # line. Unanchored cases lead because they block until a re-bless,
    # not until the code is faster.
    def report(regressions, missing)
      if regressions.empty? && missing.empty?
        return puts "gate: clean — every gated case anchored, none past the +10% floor and noise band."
      end

      missing.each { |row| puts "  NO ANCHOR  #{row.suite}/#{row.label} (#{row.metric}) — re-bless required" }
      regressions.each { |finding| puts "  REGRESSION  #{describe(finding)}" }
    end

    # Suite names the release gate covers, derived from the roster.
    def release_suites
      RELEASE_BENCHES.map { |script| File.basename(script, ".rb") }
    end

    # Compare two parsed results payloads and return the gated
    # regressions as an Array of Finding. +suites+ defaults to the
    # release roster; cases absent from the baseline are skipped.
    def compare(current, baseline, suites: release_suites)
      suites.flat_map do |suite|
        base_rows = index(baseline.dig("suites", suite))
        index(current.dig("suites", suite)).filter_map do |label, row|
          base = base_rows[label]
          base && finding_for(suite, label, row, base)
        end
      end
    end

    # Gated cases present in +current+ but absent from +baseline+, as an
    # Array of Unanchored. A case is gated when it carries a gate metric
    # (+wall_time+ or +ips+); cold-path rows (+seconds+ only) are not
    # gated, so their absence is not a failure.
    def unanchored(current, baseline, suites: release_suites)
      suites.flat_map do |suite|
        base_rows = index(baseline.dig("suites", suite))
        index(current.dig("suites", suite)).filter_map do |label, row|
          metric = gate_metric(row)
          Unanchored.new(suite, label, metric) if metric && !base_rows.key?(label)
        end
      end
    end

    # One-line human description of a Finding.
    def describe(finding)
      format("%<suite>s/%<label>s  %<metric>s  +%<delta>.1f%% (band ±%<band>.1f%%)",
             suite: finding.suite, label: finding.label, metric: finding.metric,
             delta: finding.delta_pct, band: finding.band_pct)
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
