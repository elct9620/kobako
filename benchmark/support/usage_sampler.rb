# frozen_string_literal: true

require_relative "stats"

module Kobako
  module Bench
    # Samples the {Kobako::Execution#usage} each block invocation returns
    # and reduces to the median +wall_time+ / +memory_peak+, so a single
    # GC-inflated invocation does not become the recorded per-invocation
    # budget. Drives {Runner#case_with_usage}; the bare
    # {Runner#annotate_usage!} point sample stays for memory, whose
    # sandbox accumulates state its block cannot re-run against.
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
      # +wall_time_sd+ and +wall_time_samples+ ride along so the release
      # gate can build a noise band on +wall_time+. The count is half of
      # that band: a deviation says how far one invocation strays, only
      # the count says how far their median can. Each block call returns
      # its own +Execution+ to read usage from.
      def sample(&block)
        samples = drive(&block)
        walls = samples.map(&:wall_time)
        { wall_time: Stats.median(walls), wall_time_sd: Stats.stdev(walls), wall_time_samples: walls.size,
          memory_peak: Stats.median(samples.map(&:memory_peak)).round }
      end

      # Drive the block until the CPU budget elapses, clamped to the
      # sample bounds, and return the usages collected.
      def drive
        samples = []
        deadline = cpu_now + BUDGET
        samples << yield.usage until samples.size >= MAX_SAMPLES || (samples.size >= MIN_SAMPLES && cpu_now >= deadline)
        samples
      end

      def cpu_now
        Process.clock_gettime(Process::CLOCK_PROCESS_CPUTIME_ID)
      end
    end
  end
end
