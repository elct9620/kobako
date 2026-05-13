# frozen_string_literal: true

require "benchmark/ips"
require "fileutils"
require "json"
require "time"

require_relative "env"

module Kobako
  module Bench
    # Thin wrapper around benchmark-ips that:
    #
    #   - groups cases under a single benchmark suite name (e.g. "cold_start")
    #   - records iterations-per-second and standard deviation for every case
    #   - writes the result set into `benchmark/results/<date>-<sha>.json`
    #     so two runs can be diffed without re-parsing terminal output
    #
    # Each `#case` block is a benchmark-ips probe; the runner does not
    # try to be clever about warmup or iter counts beyond what
    # benchmark-ips offers. Per-suite tuning (shorter warmup, fewer
    # iterations) is set via the `time:` / `warmup:` arguments at
    # construction time so a smoke run can pass `time: 1, warmup: 1`.
    class Runner
      ROOT = File.expand_path("../..", __dir__)
      RESULTS_DIR = File.join(ROOT, "benchmark", "results")

      attr_reader :suite, :results

      # +suite+ identifies the benchmark group (matches the filename
      # under benchmark/, minus the .rb extension). +time+ and
      # +warmup+ are forwarded to Benchmark.ips.
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
      def case(label, &)
        report = measure(label, &)
        @results << record(label, report)
      end

      # Record a one-shot wall-clock measurement. Used for cold-path
      # timings (e.g. first +Sandbox.new+ in a process pays for
      # Engine and Module init) where iterating under +benchmark-ips+
      # would only ever observe the warm path. +label+ identifies the
      # observation; the block is executed exactly once and its
      # elapsed seconds are recorded.
      def one_shot(label)
        started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
        yield
        elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - started
        @results << { label: label, seconds: elapsed, mode: "one_shot" }
        puts format("%<label>-30s %<ms>10.3f ms (one-shot)", label: label, ms: elapsed * 1000)
      end

      # Persist the collected results to
      # `benchmark/results/<date>-<sha>.json`. Returns the absolute
      # path. Existing files for the same `(date, sha)` pair are
      # merged so multiple `Runner` instances within one invocation
      # share a single output file.
      def write!
        FileUtils.mkdir_p(RESULTS_DIR)
        path = result_path
        payload = load_payload(path)
        payload["suites"][@suite] = @results.map { |r| r.transform_keys(&:to_s) }
        File.write(path, JSON.pretty_generate(payload))
        path
      end

      private

      def measure(label, &block)
        suite = Benchmark.ips do |x|
          x.config(time: @time, warmup: @warmup)
          x.report(label, &block)
        end
        suite.entries.last
      end

      def record(label, report)
        {
          label: label,
          ips: report.ips,
          ips_sd: report.ips_sd,
          iterations: report.iterations,
          cycles: report.measurement_cycle
        }
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
