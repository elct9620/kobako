# frozen_string_literal: true

require "etc"
require "rbconfig"

module Kobako
  # Quality-pipeline tooling — not loaded by the gem itself. The
  # `benchmark/support/*` helpers exist purely to drive the SPEC.md
  # "Regression benchmarks" suite. See `benchmark/` for the entry
  # points.
  module Bench
    # Captures the execution environment of a benchmark run so two
    # baseline files can be compared without ambiguity. Result JSON
    # written by {Runner} embeds this snapshot as the `env` field.
    module Env
      module_function

      # Returns the snapshot as a plain Hash suitable for JSON. +yjit_enabled+
      # is recorded so two baselines captured under different JIT states can be
      # compared without ambiguity; YJIT itself is not turned on by the
      # benchmark suite — the runner respects whatever the invoking process
      # chose (e.g. +RUBY_YJIT_ENABLE=1+ or +ruby --yjit+).
      def snapshot
        HOST.merge(
          yjit_enabled: yjit_enabled?,
          git_sha: git_sha,
          captured_at: Time.now.utc.iso8601,
          load_avg: load_avg,
          power_source: power_source,
          cpu_probe_spread_pct: cpu_probe_spread_pct
        )
      end

      # Process-invariant host fields lifted out of {.snapshot} so the
      # per-call hash stays focused on the moving parts (YJIT state, git
      # SHA, capture timestamp) and the snapshot method body stays under
      # +Metrics/MethodLength+.
      HOST = {
        ruby_version: RUBY_VERSION,
        ruby_platform: RUBY_PLATFORM,
        ruby_engine: RUBY_ENGINE,
        host_os: RbConfig::CONFIG["host_os"],
        host_cpu: RbConfig::CONFIG["host_cpu"],
        processors: Etc.nprocessors
      }.freeze

      # +true+ iff YJIT is active in the current Ruby process. Returns +false+
      # on Ruby builds that do not ship YJIT (older mruby, TruffleRuby, JRuby)
      # so the field is always boolean rather than +nil+.
      def yjit_enabled?
        defined?(RubyVM::YJIT) && RubyVM::YJIT.enabled?
      end

      # Best-effort short git SHA. Returns the literal string
      # "unknown" outside a git checkout — benchmarks must remain
      # runnable from an unpacked gem.
      def git_sha
        sha = `git rev-parse --short HEAD 2>/dev/null`.strip
        sha.empty? ? "unknown" : sha
      end

      # Best-effort 1-minute load average ("unknown" where neither
      # /proc nor sysctl answers) — recorded so a run taken on a busy
      # machine is recognisable after the fact.
      def load_avg
        return File.read("/proc/loadavg").split.first.to_f if File.readable?("/proc/loadavg")

        avg = `sysctl -n vm.loadavg 2>/dev/null`[/[\d.]+/]
        avg ? avg.to_f : "unknown"
      end

      # "ac" / "battery" via pmset on macOS, "unknown" elsewhere.
      # Battery-powered runs throttle differently and should not be
      # compared against AC baselines.
      def power_source
        out = `pmset -g batt 2>/dev/null`
        return "unknown" if out.empty?

        out.include?("AC Power") ? "ac" : "battery"
      end

      # Spread between two back-to-back runs of a fixed pure-CPU probe,
      # as a percentage — the session's own noise floor. CPU-time
      # measurement still sees frequency scaling, so a large spread
      # flags the whole run as taken in an unstable machine state.
      # Memoised: the value describes the process, not each call.
      def cpu_probe_spread_pct
        @cpu_probe_spread_pct ||= begin
          first = probe_seconds
          second = probe_seconds
          ((first - second).abs / [first, second].max * 100).round(2)
        end
      end

      # CPU seconds of one fixed probe workload (pure integer loop, no
      # allocation, ~0.1s) — comparable across runs by construction.
      def probe_seconds
        started = Process.clock_gettime(Process::CLOCK_PROCESS_CPUTIME_ID)
        x = 0
        5_000_000.times { |i| x ^= i }
        Process.clock_gettime(Process::CLOCK_PROCESS_CPUTIME_ID) - started
      end
    end
  end
end
