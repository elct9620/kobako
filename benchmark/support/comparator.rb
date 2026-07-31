# frozen_string_literal: true

require_relative "roster"

module Kobako
  module Bench
    # Pure release-gate judgment over two parsed results payloads: which
    # gated cases regressed past the anchor, and which gated cases one
    # payload carries that the other lacks. No IO — {Gate} owns the
    # run/anchor file handling and the abort/exit shell around these.
    #
    # The floor (+FLOOR_PCT+) is the conservative backstop SPEC.md names;
    # the noise band (+SIGMA+ combined standard errors) can only WIDEN
    # the bar on high-variance rows, never narrow it below the floor. So
    # the gate never flags more than a bare +10% rule would — it only
    # suppresses flags on demonstrably noisy rows (the 512 KiB guest-return
    # host wrapper being the motivating false positive).
    #
    # Two dispersions feed that band and the wider governs: the standard
    # error of the median one run recorded, and the caller-supplied
    # between-run estimate ({History}), which is the transient a row shows
    # between processes. The first cannot see the second, so on the rows
    # where they diverge the within-run half alone produces a standing
    # false alarm.
    #
    # Metric per row follows the gate policy: rows carrying +wall_time+
    # (sandbox-driven) gate on +wall_time+ — the machine-load-insensitive
    # guest budget, where a slowdown shows as a larger value; pure host
    # rows gate on the median +ips+, where a slowdown shows as a smaller
    # value. A +seconds+ row carries neither and is outside the gate even
    # inside a gated suite: recording one is how a probe declares the
    # figure is not a release commitment ({OneShot} owns what that
    # covers; SPEC.md's Regression benchmarks section pins which figures
    # are commitments).
    module Comparator
      FLOOR_PCT = 10.0
      SIGMA = 2.0
      # A median's standard error, as a multiple of the sample deviation
      # over the root of the sample count. The band guards a median, so
      # the deviation a row carries is scaled to it rather than read as
      # the spread of one observation.
      MEDIAN_SE = 1.2533
      # Ceiling on the archive-derived half of the band. The archive is a
      # committed input that grows by ordinary means, so without a ceiling a
      # row's bar rises with whatever was archived — and the anchor's own
      # "moves only by a deliberate re-bless" rule would not cover it. Three
      # times the floor: past that the anchor cannot say anything useful
      # about the row, so widening stops rather than turning the row off
      # quietly. {Gate} names every row the archive widens, capped or not.
      MAX_ARCHIVE_BAND_PCT = 30.0

      # One gated regression that cleared the floor and the noise band.
      class Finding < Data.define(:suite, :label, :metric, :baseline, :current, :delta_pct, :band_pct)
      end

      # One gated row whose bar comes from the archive rather than from the
      # dispersion its own run recorded. Reported whether or not it
      # regressed: it is the standing looseness a reader needs to see.
      class Widening < Data.define(:suite, :label, :metric, :recorded_pct, :archive_pct, :capped)
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
      # +history+ maps +[suite, label, metric]+ to the row's between-run
      # move (see {History}); rows it does not carry gate on the recorded
      # deviation alone.
      def compare(current, baseline, suites: release_suites, history: {})
        map_run_rows(current, baseline, suites) do |suite, label, row, base_rows|
          base = base_rows[label]
          base && finding_for(suite, label, row, base, history)
        end
      end

      # Gated cases present in +current+ but absent from +baseline+, as an
      # Array of MissingCase. A case is gated when it carries a gate metric
      # (+wall_time+ or +ips+); +seconds+-only rows are not gated, so
      # their absence is not a failure. {Gate} calls this both
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
      def finding_for(suite, label, row, base, history)
        metric = gate_metric(row)
        return nil unless metric

        cur_c = central(row, metric)
        base_c = central(base, metric)
        return nil if cur_c.zero? || base_c.zero?

        delta = regression_pct(metric, base_c, cur_c)
        band = band_for(row, base, metric, history[[suite, label, metric]])
        return nil unless delta > FLOOR_PCT && delta > band

        Finding.new(suite, label, metric, base_c, cur_c, delta, band)
      end

      # The band a regression must clear on top of the floor: the wider
      # of what one run's own sampling showed and what the archive shows
      # the row doing between runs. {Report} reads it through here too, so
      # the summary a human arbitrates from cannot disagree with the verdict.
      def band_for(row, base, metric, move)
        [noise_band(row, base, metric), between_run_band(move)].max
      end

      # SIGMA combined relative standard errors, as a percentage — the
      # half-width of the band a regression must clear on top of the floor.
      # Errors propagate in quadrature across the two runs.
      def noise_band(row, base, metric)
        SIGMA * Math.sqrt((median_se(row, metric)**2) + (median_se(base, metric)**2)) * 100
      end

      # Relative standard error of the median +row+ records under +metric+.
      # A row recording one observation keeps its deviation whole, which
      # is what a deviation is worth there.
      def median_se(row, metric)
        central, deviation, samples = central_sd(row, metric)
        return 0.0 if central.zero?

        scale = samples > 1 ? MEDIAN_SE / Math.sqrt(samples) : 1.0
        deviation * scale / central
      end

      # SIGMA times the row's typical between-run move, as a percentage,
      # clamped to MAX_ARCHIVE_BAND_PCT. The move is already the scale of a
      # run-to-run difference, so it needs no quadrature — it is the
      # counterpart of {noise_band}'s combined term, not of one run's
      # deviation. Zero when the archive carries no estimate, leaving the
      # recorded deviation to govern.
      def between_run_band(move)
        [SIGMA * move.to_f * 100, MAX_ARCHIVE_BAND_PCT].min
      end

      # Every gated row the archive, not the row's own run, sets the bar
      # for — as an Array of Widening. {Gate} prints these on a clean pass
      # too: a row gating on an archive band is one the floor no longer
      # governs, and archiving a run captured on a loaded machine is how
      # that happens without anyone deciding it.
      def archive_widened(current, baseline, suites: release_suites, history: {})
        map_run_rows(current, baseline, suites) do |suite, label, row, base_rows|
          base = base_rows[label]
          base && widening_for(suite, label, row, base, history)
        end
      end

      # Build a Widening when the archive governs +row+'s band, else nil.
      # It governs only when it beats both the row's own dispersion and the
      # floor: under the floor the bar is the floor either way, so a wider
      # archive term there changes no verdict and naming it would bury the
      # rows where it does.
      def widening_for(suite, label, row, base, history)
        metric = gate_metric(row)
        return nil unless metric

        move = history[[suite, label, metric]]
        archive = between_run_band(move)
        recorded = noise_band(row, base, metric)
        return nil unless archive > recorded && archive > FLOOR_PCT

        Widening.new(suite, label, metric, recorded, archive, archive >= MAX_ARCHIVE_BAND_PCT)
      end

      # +wall_time+ when present (sandbox-driven), else +ips+, else nil
      # — a +seconds+ row carries no gate metric, which is what puts it
      # outside the gate (see the module doc).
      def gate_metric(row)
        return :wall_time if row.key?("wall_time")

        :ips if row["ips"]
      end

      # The row's central value, its deviation, and the samples that
      # central value was reduced from. A capture predating the count
      # reads as one sample, so an old anchor keeps the band it had.
      def central_sd(row, metric)
        if metric == :wall_time
          return [row["wall_time"].to_f, row["wall_time_sd"].to_f, samples(row["wall_time_samples"])]
        end

        [row["ips"].to_f, row["ips_sd"].to_f, samples(row["cycles"])]
      end

      # +row+'s central value alone, for the callers that only compare
      # levels.
      def central(row, metric)
        central_sd(row, metric).first
      end

      def samples(recorded)
        count = recorded.to_i
        count.positive? ? count : 1
      end

      # Regression as a positive percentage: +ips+ slows when it drops,
      # +wall_time+ slows when it rises. An improvement yields a negative
      # value, which the floor check rejects.
      def regression_pct(metric, base, cur)
        metric == :wall_time ? (cur - base) / base * 100 : (base - cur) / base * 100
      end
    end
  end
end
