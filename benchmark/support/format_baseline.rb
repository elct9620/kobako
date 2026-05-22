# frozen_string_literal: true

# Formats a benchmark/results/<date>-<sha>.json baseline as a flat
# table of derived, human-readable values. README.md's per-suite
# tables are populated from this script's output so future baseline
# refreshes do not require hand-arithmetic on the raw ips / seconds
# numbers — every conversion below has exactly one definition.
#
# Conversions:
#
#   - ips cases  → mean time per op in µs (1e6 / ips) or ms when
#                  ips < 1000. ±sd is reported as percentage of mean.
#   - one_shot   → CPU seconds → ms.
#   - memory     → rss_kb → MB (rss_kb / 1024.0); deltas pass through.
#   - concurrent → seconds → ms; ops_per_sec passed through.
#   - usage      → wall_time seconds → ns / µs / ms (matches
#                  +time_per_op+'s thresholds); memory_peak bytes →
#                  B / KiB / MiB. Sandbox-driven ips rows carry both
#                  alongside the throughput number.
#
# Usage:
#
#   bundle exec ruby benchmark/support/format_baseline.rb \
#       benchmark/results/<date>-<sha>.json
#
# Defaults to the most recently modified results/*.json when no
# argument is given.

require "json"

module Kobako
  module Bench
    # Pure-data conversions from the runner's raw JSON shape to the
    # units README.md surfaces. Each helper has exactly one place to
    # change if the unit convention shifts.
    module Units
      module_function

      # Time per op derived from an ips number. Routes through
      # {format_seconds} so the magnitude bucket is identical to the
      # one B-35 +wall_time+ rendering uses — readers can compare
      # ips-derived per-op cost against the guest export portion
      # without mental unit-juggling.
      def time_per_op(ips)
        format_seconds(1.0 / ips)
      end

      # Standard deviation as a percentage of the mean. The runner
      # records the absolute sd; we want a relative number for the
      # README's ±x.x% form.
      def sd_pct(mean, deviation)
        return "0.0%" if mean.zero?

        format("±%.1f%%", (deviation.to_f / mean) * 100)
      end

      # CPU one_shot seconds → ms. The README quotes one_shot timings
      # at ms precision; ns / µs are below the runner's measurement
      # resolution for single-execution blocks.
      def seconds_to_ms(seconds)
        format("%.3f ms", seconds * 1000)
      end

      # KB → MB with one decimal. The memory suite samples RSS in KB
      # (`ps -o rss=`); the README quotes MB for capacity planning.
      def kb_to_mb(kilobytes)
        format("%.1f MB", kilobytes / 1024.0)
      end

      # Ops/sec passed through; the concurrent suite's #thread/s and
      # #eval/s figures are already in the right unit.
      def ops_per_sec(ops_per_sec)
        return format("%.0f ops/s", ops_per_sec) if ops_per_sec < 10_000

        format("%.1fk ops/s", ops_per_sec / 1000.0)
      end

      # Seconds → ns / µs / ms. Single definition of the magnitude
      # buckets shared by {time_per_op} (ips-derived per-op cost) and
      # the B-35 +wall_time+ rendering in {BaselineFormatter#format_usage}:
      # +< 1 µs+ renders as ns; +1 µs..1 ms+ as µs; +>= 1 ms+ as ms.
      def format_seconds(seconds)
        return format("%.0f ns", seconds * 1_000_000_000) if seconds < 0.000_001
        return format("%.1f µs", seconds * 1_000_000) if seconds < 0.001

        format("%.2f ms", seconds * 1000)
      end

      # Per-invocation +memory_peak+ ({SPEC.md B-35}) → B / KiB / MiB.
      # +0+ is common for cases that don't grow guest linear memory
      # (nil-returning evals, RPC roundtrips); the B form preserves
      # that signal instead of rounding it away.
      def memory_peak(bytes)
        return format("%.1f MiB", bytes / 1_048_576.0) if bytes >= 1_048_576
        return format("%.1f KiB", bytes / 1024.0) if bytes >= 1024

        format("%d B", bytes)
      end
    end

    # Walks a parsed runner JSON document and emits one row per case
    # into +stdout+ as `suite | label | derived_value | meta`. Used
    # by +benchmark/support/format_baseline.rb+ (the CLI shim) and
    # the README.md regeneration workflow.
    class BaselineFormatter
      def initialize(payload)
        @payload = payload
      end

      def emit(io)
        emit_header(io)
        @payload["suites"].sort.each do |suite, cases|
          cases.each { |entry| emit_row(io, suite, entry) }
        end
      end

      private

      def emit_header(io)
        env = @payload["env"] || {}
        io.puts "# baseline #{env["captured_at"]} @ #{env["git_sha"]}"
        io.puts "# #{env["ruby_platform"]} ruby=#{env["ruby_version"]} yjit=#{env["yjit_enabled"]}"
        io.puts
        io.puts ["suite", "label", "value", "±sd / meta"].join("\t")
      end

      def emit_row(io, suite, entry)
        value, meta = format_entry(entry)
        io.puts [suite, entry["label"], value, meta].join("\t")
      end

      def format_entry(entry)
        return format_ips(entry) if entry["ips"]
        return format_memory(entry) if entry.key?("rss_kb")
        return format_concurrent(entry) if entry["mode"] == "concurrent"
        return [Units.seconds_to_ms(entry["seconds"]), "one_shot"] if entry["mode"] == "one_shot"

        ["", entry.to_json]
      end

      def format_ips(entry)
        value = Units.time_per_op(entry["ips"])
        meta_parts = ["#{Units.sd_pct(entry["ips"], entry["ips_sd"])}, n=#{entry["cycles"]}"]
        meta_parts << format_usage(entry) if entry.key?("wall_time")
        [value, meta_parts.join(" | ")]
      end

      def format_memory(entry)
        value = Units.kb_to_mb(entry["rss_kb"])
        deltas = entry.select { |k, _| k.end_with?("_kb") && k != "rss_kb" }
                      .map { |k, v| "#{k.sub(/_kb\z/, "")}=#{Units.kb_to_mb(v)}" }
        meta_parts = [deltas.join(", ")].reject(&:empty?)
        meta_parts << format_usage(entry) if entry.key?("wall_time")
        [value, meta_parts.join(" | ")]
      end

      # Render the B-35 +wall_time+ / +memory_peak+ pair that
      # {Runner#case_with_usage} and the memory suite's +record+
      # helper fold into ips and memory rows.
      def format_usage(entry)
        "wall=#{Units.format_seconds(entry["wall_time"])} mem=#{Units.memory_peak(entry["memory_peak"])}"
      end

      def format_concurrent(entry)
        if entry["ops_per_sec"]
          [Units.ops_per_sec(entry["ops_per_sec"]), "wall=#{Units.seconds_to_ms(entry["seconds"])}"]
        elsif entry["ratio"]
          [format("%.2fx", entry["ratio"]), "baseline=#{format("%.3f", entry["baseline_ms"])} ms"]
        elsif entry["seconds"]
          [Units.seconds_to_ms(entry["seconds"]), entry["mode"]]
        else
          ["", entry.to_json]
        end
      end
    end
  end
end

if $PROGRAM_NAME == __FILE__
  results_dir = File.expand_path("../results", __dir__)
  path = ARGV[0] || Dir[File.join(results_dir, "*.json")].max_by { |f| File.mtime(f) }
  abort "no baseline JSON found" unless path && File.exist?(path)

  payload = JSON.parse(File.read(path))
  Kobako::Bench::BaselineFormatter.new(payload).emit($stdout)
end
