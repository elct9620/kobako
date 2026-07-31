# frozen_string_literal: true

require "fileutils"
require "json"

require_relative "comparator"
require_relative "history"
require_relative "paths"

module Kobako
  module Bench
    # Release-gate runner: resolves the run and the committed anchor
    # baseline, delegates the judgment to {Comparator}, prints the outcome,
    # and aborts (non-zero exit) on any gated regression or unanchored case
    # so the release pipeline fails. The anchor (+benchmark/baseline.json+)
    # is fixed, not the previous run, so sub-threshold drift accumulates
    # against it instead of resetting each release; it advances only by
    # {bless!}. See the Regression benchmarks section of SPEC.md.
    module Gate
      ANCHOR_PATH = Paths::BASELINE_ANCHOR
      RESULTS_GLOB = Paths::RESULTS_GLOB

      module_function

      # Resolve the run and the anchor, judge the run via {Comparator}, and
      # abort on any blocking issue. The rake task delegates here so the
      # .rake stays DSL.
      def gate!(current = nil, baseline = nil)
        current, baseline = locate(current, baseline)
        run = load_payload(current)
        anchor = load_payload(baseline)
        puts "gate: #{File.basename(current)} vs anchor #{File.basename(baseline)}"
        judge(run, anchor, Comparator.release_suites - note_remethoded(run, anchor))
      end

      # Judge +run+ against +anchor+ over +judged+ and abort on anything
      # blocking. A remethoded suite is left out rather than flagged: its
      # delta is the distance between two different measurements, so a
      # regression there would be a false positive by construction.
      # Coverage still spans every suite — a case the anchor lacks is
      # missing whatever method produced it.
      def judge(run, anchor, judged)
        history = History.dispersion(methods: run.fetch("methods", {}))
        enforce(Comparator.compare(run, anchor, suites: judged, history: history),
                Comparator.gated_absences(run, anchor), Comparator.gated_absences(anchor, run),
                Comparator.archive_widened(run, anchor, suites: judged, history: history))
      end

      # Re-bless the anchor baseline from +run+ (a results JSON path),
      # replacing +benchmark/baseline.json+. This is the only way the
      # anchor moves; the cumulative budget then resets to the blessed
      # numbers, so the accepted shift and its justification must be
      # recorded in the benchmark README's "What changed" section.
      def bless!(run)
        raise "bench:bless needs a results JSON to bless as the anchor" unless run
        raise "bench:bless: #{run} does not exist" unless File.exist?(run)
        raise "bench:bless: #{run} is not a benchmark results JSON" unless results_payload?(run)

        FileUtils.cp(run, ANCHOR_PATH)
        puts "blessed anchor: #{File.basename(run)} -> #{File.basename(ANCHOR_PATH)}"
        puts "record the accepted shift and why in benchmark/README.md \"What changed\" before committing."
      end

      # Resolve [current, anchor]: +current+ defaults to the newest run
      # under benchmark/results/, +baseline+ to the committed anchor
      # (benchmark/baseline.json). Either may be given explicitly.
      def resolve(current, baseline)
        current ||= Dir[RESULTS_GLOB].max_by { |path| File.mtime(path) }
        [current, baseline || ANCHOR_PATH]
      end

      # Resolve the pair and abort with a remediation hint when the run or
      # the anchor is absent, rather than letting the later read raise a
      # bare Errno::ENOENT.
      def locate(current, baseline)
        current, baseline = resolve(current, baseline)
        abort "bench:gate: no run to gate; run `rake bench` first." unless current && File.exist?(current)
        abort "bench:gate: no anchor at #{ANCHOR_PATH}; run `rake bench:bless` first." unless File.exist?(baseline)

        [current, baseline]
      end

      def load_payload(path)
        JSON.parse(File.read(path))
      end

      # True when +path+ parses as a benchmark results payload (a JSON
      # object carrying a "suites" map). Guards {bless!} so a malformed or
      # unrelated file cannot become the anchor and crash the next gate.
      def results_payload?(path)
        parsed = JSON.parse(File.read(path))
        parsed.is_a?(Hash) && parsed.key?("suites")
      rescue JSON::ParserError
        false
      end

      # Report findings then abort on any blocking issue. Regressions and
      # unanchored cases block; a case in the anchor but absent from the run
      # (+dropped+) is only a NOTE, since the next re-bless records the drop.
      def enforce(regressions, missing, dropped, widened = [])
        report(regressions, missing, dropped, widened)
        problems = regressions.size + missing.size
        return if problems.zero?

        abort "gate: #{problems} gated issue(s) — arbitrate real-vs-noise with " \
              "`rake bench:confirm[<last release>]` before a re-bless or release."
      end

      # Print dropped-case NOTEs (non-blocking), the rows the archive
      # widened, then the unanchored cases and gated regressions, or a
      # clean-pass line when neither blocks.
      def report(regressions, missing, dropped, widened = [])
        note_dropped(dropped)
        note_widened(widened)
        if regressions.empty? && missing.empty?
          return puts "gate: clean — every gated case anchored, none past the +10% floor and noise band."
        end

        missing.each { |row| puts "  NO ANCHOR  #{row.suite}/#{row.label} (#{row.metric}) — re-bless required" }
        regressions.each { |finding| puts "  REGRESSION  #{describe(finding)}" }
      end

      # NOTE: the gated rows whose bar the archive raised above their own
      # recorded dispersion. Non-blocking, and printed on a clean pass too —
      # a pass on an archive-widened row says less than a pass on a row the
      # floor still governs, and only saying so makes that legible.
      def note_widened(widened)
        return if widened.empty?

        capped = widened.count(&:capped)
        widest = widened.max_by(&:archive_pct)
        puts "  NOTE  #{widened.size} gated row(s) gate on an archive-widened band, widest " \
             "#{widest.suite}/#{widest.label} ±#{format("%.1f", widest.archive_pct)}% " \
             "(own run recorded ±#{format("%.1f", widest.recorded_pct)}%)" \
             "#{"; #{capped} at the #{Comparator::MAX_ARCHIVE_BAND_PCT}% ceiling" unless capped.zero?}"
      end

      # NOTE: and return the gated suites the run measured by a different
      # method than the anchor did. Non-blocking: a method change is a
      # deliberate act the re-bless absorbs, and blocking would fail the
      # release on a delta that is not a performance reading.
      def note_remethoded(run, anchor)
        changed = Comparator.release_suites.reject do |suite|
          (run.dig("methods", suite) || 1) == (anchor.dig("methods", suite) || 1)
        end
        return changed if changed.empty?

        puts "  NOTE  measured by a different method than the anchor: #{changed.join(", ")} — " \
             "left out of the comparison, since the delta would be the distance between two " \
             "different measurements; the re-bless is what puts them back under judgment"
        changed
      end

      # NOTE: each gated case the anchor carries that the run no longer
      # emits; non-blocking, since the next re-bless records the drop.
      def note_dropped(dropped)
        dropped.each do |row|
          puts "  NOTE  #{row.suite}/#{row.label} (#{row.metric}) in anchor but absent from run — re-bless to drop it"
        end
      end

      # One-line human description of a Comparator::Finding.
      def describe(finding)
        format("%<suite>s/%<label>s  %<metric>s  +%<delta>.1f%% (band ±%<band>.1f%%)",
               suite: finding.suite, label: finding.label, metric: finding.metric,
               delta: finding.delta_pct, band: finding.band_pct)
      end
    end
  end
end
