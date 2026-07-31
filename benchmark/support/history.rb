# frozen_string_literal: true

require "json"

require_relative "paths"
require_relative "stats"

module Kobako
  module Bench
    # Between-run dispersion per case, read from the archived runs under
    # benchmark/results/. The Comparator's other half is derived from what
    # one process observed, which cannot see the allocator and
    # frequency-scaling transients that move a row between processes. The large-payload codec rows measure 15-25%
    # between runs against an 8-9% within-run band, and flagged twice in a
    # row on a codec whose hot path had not changed.
    #
    # The estimate is the MEDIAN relative move between consecutive runs,
    # not the spread of the levels: the archive spans months of accepted
    # optimisations, and a level-based estimate would read each of those
    # steps as noise and widen the band on exactly the rows that measure
    # cleanly. A step is one outlier among the moves, so the median holds
    # at the quiet level around it.
    #
    # The estimate only ever widens the bar — {Comparator} takes the wider
    # of the two bands and the +10% floor stands underneath both, which is
    # what keeps a quiet archive's near-zero estimate from tightening the
    # gate rather than loosening it.
    module History
      # Runs to look back over, so a row's estimate tracks its current
      # character rather than the whole archive's.
      WINDOW = 10
      # Consecutive moves a row needs before its median means anything.
      MIN_MOVES = 3

      module_function

      # Median relative between-run move per +[suite, label, metric]+,
      # over the most recent {WINDOW} archived runs. Rows appearing in
      # fewer than {MIN_MOVES}+1 of them are absent, leaving the gate on
      # the within-run band alone. +methods+ is the measurement-method
      # version per suite the caller is estimating for: once a suite has
      # accumulated enough runs under it, only those runs are read, since
      # a move across two methods measures the change of method rather
      # than the machine. Until then the estimate over every method
      # stands, because how far a row moves between processes is a
      # property of the machine that survives a reordering — dropping it
      # the moment a version is bumped would leave the noisiest rows on
      # the bare floor, which is the standing false alarm this estimate
      # exists to prevent.
      def dispersion(glob = Paths::RESULTS_GLOB, methods: {})
        runs = recent_runs(glob)
        estimate(series(runs, {})).merge(estimate(series(runs, methods)))
      end

      # Median move per key, for the rows showing enough consecutive moves
      # to have one.
      def estimate(series)
        series.filter_map do |key, values|
          moves = relative_moves(values)
          [key, Stats.median(moves)] if moves.size >= MIN_MOVES
        end.to_h
      end

      # The most recent {WINDOW} archived runs, oldest first. Ordered by
      # the capture stamp rather than the filename, which carries only a
      # date and a sha and cannot order two runs from the same day.
      def recent_runs(glob)
        Dir[glob].map { |path| JSON.parse(File.read(path)) }
                 .sort_by { |run| run.dig("env", "captured_at").to_s }
                 .last(WINDOW)
      end

      # Each case's values in run order, keyed by suite, label, and the
      # metric they were read under. A row carrying both metrics lands
      # under each, so the caller finds the key for whichever metric it
      # gates that row on without sharing this module's notion of which.
      def series(runs, methods = {})
        runs.each_with_object(Hash.new { |h, k| h[k] = [] }) do |run, acc|
          run.fetch("suites", {}).each do |suite, rows|
            next unless same_method?(run, suite, methods)

            rows.each { |row| collect_metrics(acc, suite, row) }
          end
        end
      end

      # True when +run+ captured +suite+ under the method version the
      # caller is estimating for. A payload predating the stamp reads as
      # version 1 on both sides, which is what every suite was before one
      # of them moved.
      def same_method?(run, suite, methods)
        (run.dig("methods", suite) || 1) == methods.fetch(suite, 1)
      end

      # Append +row+'s value under each metric it carries.
      def collect_metrics(acc, suite, row)
        %i[ips wall_time].each do |metric|
          value = row[metric.to_s]
          acc[[suite, row["label"], metric]] << value.to_f if value
        end
      end

      # Relative move between each consecutive pair, as a fraction.
      def relative_moves(values)
        values.each_cons(2).filter_map do |before, after|
          ((after - before) / before).abs if before.positive?
        end
      end
    end
  end
end
