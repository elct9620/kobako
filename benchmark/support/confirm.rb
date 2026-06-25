# frozen_string_literal: true

require "json"
require "open3"
require "tmpdir"

require_relative "baseline_wasm"
require_relative "guest"
require_relative "paths"
require_relative "runner"

module Kobako
  module Bench
    # Stage-2 arbiter behind the release gate: judges a suspected
    # regression against a released Guest Binary by paired alternation on
    # one machine — short adjacent arms survive the minute-scale machine
    # transients that make cross-day comparisons lie (see the noise
    # section of benchmark/README.md). +bench:gate+ stays the cheap
    # stage-1 smoke detector; this is the judge it defers to.
    module Confirm
      SCRIPT = Paths.probe("mruby_eval")
      SUITE = "mruby_eval"
      PAIRS = 3
      THRESHOLD_PCT = 3.0
      SPREAD_LIMIT_PCT = 20.0

      # Per-label verdict: the per-pair deltas (current vs baseline, in
      # percent), their mean, and the call — +:regression+ /
      # +:improvement+ only when every pair agrees on direction AND the
      # mean clears +THRESHOLD_PCT+; +:unstable+ when the pairs spread
      # wider than +SPREAD_LIMIT_PCT+ (the machine was not quiet, so the
      # arbitration is void); else +:noise+.
      class Row < Data.define(:label, :deltas, :mean_pct, :verdict)
      end

      module_function

      # Resolve the baseline, run the paired arms, judge, and report. Each
      # arm runs the probe against a Guest Binary injected through the
      # environment, so the in-place data/kobako.wasm is never touched.
      def confirm!(ref, pairs: PAIRS)
        abort "bench:confirm needs a baseline — a released version or a wasm path." unless ref

        baseline = BaselineWasm.resolve(ref)
        report(judge(measure_pairs(baseline, Paths::DATA_WASM, pairs)))
      end

      # Pure verdict math over +samples+ (one +[baseline_rows,
      # current_rows]+ per pair, each a label=>ips map). Exposed for the
      # unit tests; no IO.
      def judge(samples)
        samples.first.first.keys.map do |label|
          deltas = samples.map { |base, cur| (cur[label] - base[label]) / base[label] * 100 }
          Row.new(label: label, deltas: deltas, mean_pct: mean(deltas), verdict: verdict_for(deltas))
        end
      end

      def verdict_for(deltas)
        return :unstable if deltas.max - deltas.min > SPREAD_LIMIT_PCT
        return :regression if unanimous?(deltas.map(&:-@))
        return :improvement if unanimous?(deltas)

        :noise
      end

      # Every delta positive and the mean past the threshold — direction
      # normalised by the caller (negated for the regression side).
      def unanimous?(deltas)
        deltas.all?(&:positive?) && mean(deltas) >= THRESHOLD_PCT
      end

      def mean(values)
        values.sum / values.size
      end

      # Alternate baseline/current so each pair is adjacent in time —
      # the property that cancels machine drift.
      def measure_pairs(baseline, current, pairs)
        Array.new(pairs) { [run_arm(baseline), run_arm(current)] }
      end

      # Run the probe suite against +wasm+ by injecting it through the
      # environment and harvesting its label=>ips rows from a throwaway
      # results directory — neither data/kobako.wasm nor benchmark/results
      # is touched.
      def run_arm(wasm)
        Dir.mktmpdir("kobako-confirm") do |dir|
          out, status = Open3.capture2(
            { Guest::ENV_KEY => wasm, Runner::RESULTS_DIR_ENV => dir },
            "bundle", "exec", "ruby", SCRIPT
          )
          raise "bench:confirm arm failed:\n#{out}" unless status.success?

          harvest(out.lines.last.strip)
        end
      end

      def harvest(path)
        JSON.parse(File.read(path)).dig("suites", SUITE).to_h { |row| [row["label"], row["ips"]] }
      end

      def report(rows)
        rows.each { |row| puts describe(row) }
        confirmed = rows.count { |row| row.verdict == :regression }
        abort "confirm: #{confirmed} label(s) regressed consistently — real, not machine noise." if confirmed.positive?

        if rows.any? { |row| row.verdict == :unstable }
          abort "confirm: inconclusive — pair spreads past ±#{SPREAD_LIMIT_PCT}%; rerun on an idle machine."
        end

        puts "confirm: noise — no label slower in all #{PAIRS} pairs past ±#{THRESHOLD_PCT}%."
      end

      def describe(row)
        format("  %<verdict>-12s %<label>-34s mean %<mean>+.1f%%  (pairs: %<deltas>s)",
               verdict: row.verdict.to_s.upcase, label: row.label, mean: row.mean_pct,
               deltas: row.deltas.map { |d| format("%+.1f%%", d) }.join(" "))
      end
    end
  end
end
