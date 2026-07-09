# frozen_string_literal: true

require_relative "stats"

module Kobako
  module Bench
    # One-shot (cold-path) measurement surface of {Runner}, mixed in so
    # the calibrated-loop machinery and the single-execution recorders
    # live in separate files. Relies on the including class for
    # +cpu_time+ and +@results+.
    module OneShot
      # Record a one-shot CPU-time measurement. +label+ identifies the
      # observation; the block is executed exactly once and the CPU
      # seconds it consumes are recorded.
      def one_shot(label, &block)
        record_one_shot(label, cpu_time(&block))
      end

      # Run the block +rounds+ times and record the MEDIAN CPU seconds.
      # Sub-millisecond warm rows are hostage to minute-scale machine
      # transients when observed once; the median across rounds is the
      # stable observation (see the noise section of benchmark/README.md).
      def one_shot_median(label, rounds:, &block)
        samples = Array.new(rounds) { cpu_time(&block) }
        record_one_shot(label, Stats.median(samples), rounds: rounds)
      end

      # Public single CPU-seconds measurement. Scripts whose rounds need
      # per-round setup outside the timer (catalog_handles 5b rebuilds a
      # table per round) collect samples here and record the median via
      # {#record_one_shot}.
      def time_once(&block)
        cpu_time(&block)
      end

      # Record an already-measured one-shot observation. +rounds+ > 1
      # marks the value as a median across that many rounds.
      def record_one_shot(label, seconds, rounds: 1)
        @results << { label: label, seconds: seconds, mode: "one_shot", rounds: rounds }
        note = rounds > 1 ? "median of #{rounds}" : "one-shot"
        puts format("%<label>-35s %<ms>10.3f ms (CPU, %<note>s)", label: label, ms: seconds * 1000, note: note)
      end
    end
  end
end
