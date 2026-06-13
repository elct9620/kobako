# frozen_string_literal: true

require_relative "stats"

module Kobako
  module Bench
    # Samples {Kobako::Sandbox#usage} across
    # repeated block invocations and reduces to the median +wall_time+ /
    # +memory_peak+, so a single GC-inflated invocation does not become
    # the recorded per-invocation budget. Drives {Runner#case_with_usage};
    # the bare {Runner#annotate_usage!} point sample stays for callers
    # (cold_start, memory) that have no re-runnable block.
    module UsageSampler
      # CPU-time budget and sample bounds for the sampling loop: run
      # until the budget elapses, clamped so a cheap case still yields a
      # stable median and a multi-millisecond case (e.g. the 100k-
      # iteration mruby loop) does not run away.
      BUDGET = 0.5
      MIN_SAMPLES = 11
      MAX_SAMPLES = 200

      module_function

      # Drive the block until the budget elapses (within the sample
      # bounds) and return the median usage as a result-row fragment.
      # +wall_time_sd+ rides along so the release gate can build a noise
      # band on +wall_time+ — the gate-correct metric for sandbox-driven
      # rows. Each block call leaves its own reading on +sandbox.usage+.
      def sample(sandbox)
        samples = []
        deadline = cpu_now + BUDGET
        until samples.size >= MAX_SAMPLES || (samples.size >= MIN_SAMPLES && cpu_now >= deadline)
          yield
          samples << sandbox.usage
        end
        walls = samples.map(&:wall_time)
        { wall_time: Stats.median(walls), wall_time_sd: Stats.stdev(walls),
          memory_peak: Stats.median(samples.map(&:memory_peak)).round }
      end

      def cpu_now
        Process.clock_gettime(Process::CLOCK_PROCESS_CPUTIME_ID)
      end
    end
  end
end
