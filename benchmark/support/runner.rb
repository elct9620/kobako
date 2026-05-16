# frozen_string_literal: true

require "fileutils"
require "json"
require "time"

require_relative "env"

module Kobako
  module Bench
    # Collects per-suite benchmark results and writes them to
    # +benchmark/results/<date>-<sha>.json+. Two measurement modes:
    #
    #   - {#case} runs a calibrated CPU-time loop and records mean ips
    #     plus sample standard deviation across measurement cycles.
    #   - {#one_shot} runs the block exactly once and records the CPU
    #     seconds spent. Used for cold-path timings (first +Sandbox.new+
    #     in a process pays for Engine + Module init) where iterating
    #     would only ever observe the warm path.
    #
    # Both modes are CPU-time based — +Process::CLOCK_PROCESS_CPUTIME_ID+
    # rather than +CLOCK_MONOTONIC+ — so scheduler / background-load noise
    # does not inflate the measurement. Multi-thread suites that
    # intentionally measure scheduling overhead keep their own
    # wall-clock helper and bypass this runner.
    class Runner
      ROOT = File.expand_path("../..", __dir__)
      RESULTS_DIR = File.join(ROOT, "benchmark", "results")

      attr_reader :suite, :results

      # +suite+ identifies the benchmark group (matches the filename
      # under benchmark/, minus the .rb extension). +time+ and +warmup+
      # are CPU-time budgets in seconds; the measurement phase ends as
      # soon as cumulative CPU time exceeds +time+.
      def initialize(suite, time: 3, warmup: 1)
        @suite = suite
        @time = time
        @warmup = warmup
        @results = []
      end

      # Run a labelled benchmark case. +label+ identifies the case
      # within the suite (e.g. "1a-sandbox-new"); the block is the
      # measured workload. The block must be deterministic and free
      # of external side effects so successive runs are comparable.
      def case(label, &block)
        iters_per_cycle = calibrate(block)
        warmup_cpu(block, iters_per_cycle)
        samples, iterations = measure_samples(block, iters_per_cycle)
        mean = samples.sum / samples.size
        sd = stdev(samples, mean)
        emit_case(label, mean, sd, iterations, samples.size)
      end

      # Record a one-shot CPU-time measurement. +label+ identifies the
      # observation; the block is executed exactly once and the CPU
      # seconds it consumes are recorded.
      def one_shot(label, &)
        elapsed = cpu_time(&)
        @results << { label: label, seconds: elapsed, mode: "one_shot" }
        puts format("%<label>-35s %<ms>10.3f ms (CPU, one-shot)", label: label, ms: elapsed * 1000)
      end

      # Persist the collected results to
      # +benchmark/results/<date>-<sha>.json+. Returns the absolute path.
      # Existing files for the same +(date, sha)+ pair are merged so
      # multiple +Runner+ instances within one invocation share a single
      # output file.
      def write!
        FileUtils.mkdir_p(RESULTS_DIR)
        path = result_path
        payload = load_payload(path)
        payload["suites"][@suite] = @results.map { |r| r.transform_keys(&:to_s) }
        File.write(path, JSON.pretty_generate(payload))
        path
      end

      private

      # Returns the current process CPU time in seconds. Includes both
      # user and system time and excludes wall-clock idle / scheduler
      # waits, which is what makes measurements stable across noisy
      # hosts.
      def cpu_now
        Process.clock_gettime(Process::CLOCK_PROCESS_CPUTIME_ID)
      end

      # Run +block+ once and return the CPU seconds spent.
      def cpu_time
        started = cpu_now
        yield
        cpu_now - started
      end

      # Pick a per-cycle iteration count that lets each measurement
      # cycle consume roughly +@time / 5+ CPU seconds — five cycles fit
      # in the @time budget, giving five samples for the SD estimate.
      # Doubles iters until the cycle hits the target; bails on
      # pathological growth so a no-op block does not run forever.
      def calibrate(block)
        target = @time.to_f / 5
        iters = 1
        loop do
          return iters if cpu_time { iters.times(&block) } >= target

          iters *= 2
          return iters if iters > (1 << 28)
        end
      end

      # Run +block+ for @warmup CPU seconds via the same +Integer#times+
      # path the measurement phase uses, discarding the results. Mirroring
      # the call shape matters: Ruby's inline caches for +iters.times(&block)+
      # are distinct from +block.call+, and warming only the latter leaves
      # the first measured case paying cold-cache costs.
      def warmup_cpu(block, iters_per_cycle)
        deadline = cpu_now + @warmup
        iters_per_cycle.times(&block) while cpu_now < deadline
      end

      # Run measurement cycles until cumulative CPU time exceeds @time.
      # Each cycle runs +iters_per_cycle+ iterations and records the
      # observed ips; the resulting array is one sample per cycle.
      def measure_samples(block, iters_per_cycle)
        samples = []
        total = 0
        deadline = cpu_now + @time
        while cpu_now < deadline
          elapsed = cpu_time { iters_per_cycle.times(&block) }
          samples << (iters_per_cycle / elapsed) if elapsed.positive?
          total += iters_per_cycle
        end
        [samples, total]
      end

      # Sample standard deviation across measurement cycles. Returns
      # 0.0 when fewer than two samples were collected (the single
      # sample has no spread to report).
      def stdev(values, mean)
        return 0.0 if values.size < 2

        Math.sqrt(values.sum { |v| (v - mean)**2 } / (values.size - 1))
      end

      def emit_case(label, ips_mean, ips_sd, iterations, cycles)
        @results << { label: label, ips: ips_mean, ips_sd: ips_sd.round,
                      iterations: iterations, cycles: cycles }
        pct = ips_mean.positive? ? (ips_sd / ips_mean * 100) : 0.0
        puts format("%<label>-35s %<ips>14s (CPU ±%<pct>.1f%%, %<n>d samples)",
                    label: label, ips: humanize_ips(ips_mean), pct: pct, n: cycles)
      end

      def humanize_ips(ips)
        return format("%.1f i/s", ips) if ips < 1_000
        return format("%.2fk i/s", ips / 1_000.0) if ips < 1_000_000

        format("%.2fM i/s", ips / 1_000_000.0)
      end

      def result_path
        env = Env.snapshot
        date = Time.now.utc.strftime("%Y-%m-%d")
        File.join(RESULTS_DIR, "#{date}-#{env[:git_sha]}.json")
      end

      def load_payload(path)
        return JSON.parse(File.read(path)) if File.exist?(path)

        { "env" => Env.snapshot.transform_keys(&:to_s), "suites" => {} }
      end
    end
  end
end
