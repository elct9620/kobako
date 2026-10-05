# frozen_string_literal: true

require_relative "stats"

module Kobako
  module Bench
    # Single-execution measurement surface of {Runner}, mixed in so the
    # calibrated-loop machinery and the single-execution recorders live
    # in separate files. Recording here is also how a probe declares the
    # figure is not a release commitment: the +seconds+ row it emits
    # carries no gate metric, so {Comparator} leaves it out even in a
    # gated suite. Under smoke each recorder runs its body once and records
    # a smoke row instead, as {Smoke} does for a calibrated case. Relies on
    # the including class for +cpu_time+, +smoke?+, and +@results+.
    module OneShot
      # Record a one-shot CPU-time measurement. +label+ identifies the
      # observation; the block is executed exactly once and the CPU
      # seconds it consumes are recorded.
      def one_shot(label, &block)
        return smoke_case(label, &block) if smoke?

        record_one_shot(label, cpu_time(&block))
      end

      # Run the block +rounds+ times and record the MEDIAN CPU seconds.
      # Sub-millisecond warm rows are hostage to minute-scale machine
      # transients when observed once; the median across rounds is the
      # stable observation (see the noise section of benchmark/README.md).
      # +setup+, when given, prepares each round outside the timer and
      # hands what it returns to the block, so a round that needs its own
      # state — catalog_handles 5b rebuilds a table — stays inside the seam.
      def one_shot_median(label, rounds:, setup: nil, &block)
        return smoke_case(label) { block.call(setup&.call) } if smoke?

        samples = Array.new(rounds) do
          prepared = setup&.call
          cpu_time { block.call(prepared) }
        end
        record_one_shot(label, Stats.median(samples), rounds: rounds)
      end

      private

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
