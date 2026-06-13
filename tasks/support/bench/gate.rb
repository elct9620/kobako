# frozen_string_literal: true

require "fileutils"
require "json"

require_relative "comparator"

module KobakoBench
  # Release-gate runner: resolves the run and the committed anchor
  # baseline, delegates the judgment to {Comparator}, prints the outcome,
  # and aborts (non-zero exit) on any gated regression or unanchored case
  # so the release pipeline fails. The anchor (+benchmark/baseline.json+)
  # is fixed, not the previous run, so sub-threshold drift accumulates
  # against it instead of resetting each release; it advances only by
  # {bless!}. See the Regression benchmarks section of SPEC.md.
  module Gate
    ANCHOR_PATH = File.expand_path("../../benchmark/baseline.json", __dir__)
    RESULTS_GLOB = File.expand_path("../../benchmark/results/*.json", __dir__)

    module_function

    # Resolve the run and the anchor, judge the run via {Comparator}, and
    # abort on any blocking issue. The rake task delegates here so the
    # .rake stays DSL.
    def gate!(current = nil, baseline = nil)
      current, baseline = locate(current, baseline)
      run = load_payload(current)
      anchor = load_payload(baseline)
      puts "gate: #{File.basename(current)} vs anchor #{File.basename(baseline)}"
      enforce(Comparator.compare(run, anchor), Comparator.gated_absences(run, anchor),
              Comparator.gated_absences(anchor, run))
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
    def enforce(regressions, missing, dropped)
      report(regressions, missing, dropped)
      problems = regressions.size + missing.size
      return if problems.zero?

      abort "gate: #{problems} gated issue(s) — arbitrate real-vs-noise with " \
            "`rake bench:confirm[<last release>]` before a re-bless or release."
    end

    # Print dropped-case NOTEs (non-blocking), then the unanchored cases
    # and gated regressions, or a clean-pass line when neither blocks.
    def report(regressions, missing, dropped)
      note_dropped(dropped)
      if regressions.empty? && missing.empty?
        return puts "gate: clean — every gated case anchored, none past the +10% floor and noise band."
      end

      missing.each { |row| puts "  NO ANCHOR  #{row.suite}/#{row.label} (#{row.metric}) — re-bless required" }
      regressions.each { |finding| puts "  REGRESSION  #{describe(finding)}" }
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
