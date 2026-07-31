# frozen_string_literal: true

require "json"

require_relative "paths"
require_relative "stats"

module Kobako
  module Bench
    # Between-run dispersion per case, read from the archived runs under
    # benchmark/results/. Exists because the Comparator's other half sees
    # one process only, and cannot price the transients that move a row
    # between them.
    #
    # The estimate is the MEDIAN relative move between consecutive runs
    # rather than the spread of the levels: the archive spans accepted
    # optimisations, and a level-based estimate would read each of those
    # steps as noise.
    module History
      # Runs to look back over, so a row's estimate tracks its current
      # character rather than the whole archive's.
      WINDOW = 10
      # Consecutive moves a row needs before its median means anything.
      MIN_MOVES = 3

      module_function

      # Median relative between-run move per +[suite, label, metric]+.
      # +methods+ names the measurement-method version the caller is
      # estimating for: same-version runs govern once a suite has enough
      # of them, since a move across two methods measures the method. The
      # estimate over every version stands until then, because a row's
      # between-process movement outlives a change in how it is measured
      # — and stranding the noisiest rows on the floor is the alarm this
      # estimate exists to prevent.
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
