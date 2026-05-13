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

      # Returns the snapshot as a plain Hash suitable for JSON.
      def snapshot
        {
          ruby_version: RUBY_VERSION,
          ruby_platform: RUBY_PLATFORM,
          ruby_engine: defined?(RUBY_ENGINE) ? RUBY_ENGINE : "ruby",
          host_os: RbConfig::CONFIG["host_os"],
          host_cpu: RbConfig::CONFIG["host_cpu"],
          processors: Etc.nprocessors,
          git_sha: git_sha,
          captured_at: Time.now.utc.iso8601
        }
      end

      # Best-effort short git SHA. Returns the literal string
      # "unknown" outside a git checkout — benchmarks must remain
      # runnable from an unpacked gem.
      def git_sha
        sha = `git rev-parse --short HEAD 2>/dev/null`.strip
        sha.empty? ? "unknown" : sha
      end
    end
  end
end
