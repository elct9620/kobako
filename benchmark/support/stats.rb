# frozen_string_literal: true

module Kobako
  module Bench
    # Pure summary statistics for the per-cycle ips / per-invocation
    # usage samples the Runner collects. Ruby's stdlib ships neither a
    # median nor a standard deviation, so the Runner has always carried
    # its own; these two functions are extracted here so the throughput
    # path and the usage-sampling path share one definition.
    module Stats
      module_function

      # Arithmetic mean of +values+ — reported alongside the median so
      # the throughput / capacity reading is available next to the
      # outlier-robust one (Google Benchmark and Criterion both surface
      # both). Returns 0.0 for an empty input.
      def mean(values)
        return 0.0 if values.empty?

        values.sum / values.size
      end

      # Median of +values+ — the central value the Runner reports.
      # Robust to a single GC-inflated sample in a way the mean is not.
      # Returns 0.0 for an empty input.
      def median(values)
        return 0.0 if values.empty?

        sorted = values.sort
        mid = sorted.size / 2
        return sorted[mid] if sorted.size.odd?

        (sorted[mid - 1] + sorted[mid]) / 2.0
      end

      # Sample standard deviation of +values+ — the dispersion reported
      # alongside the median. Returns 0.0 for fewer than two samples
      # (a single sample has no spread to report).
      def stdev(values)
        return 0.0 if values.size < 2

        mean = values.sum / values.size
        Math.sqrt(values.sum { |v| (v - mean)**2 } / (values.size - 1))
      end
    end
  end
end
