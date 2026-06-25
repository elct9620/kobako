# frozen_string_literal: true

require "json"

require_relative "gate"
require_relative "confirm"
require_relative "report"
require_relative "roster"

module Kobako
  # The surface the rake runner (+tasks/bench/+) drives. The gate /
  # confirm / report subsystem, the roster, and all path resolution sit
  # behind these verbs so the rake layer stays a thin DSL holding no
  # benchmark internals.
  module Bench
    module_function

    # Anchored release gate: compare a run against benchmark/baseline.json
    # (or explicit +current+ / +baseline+ paths).
    def gate(current = nil, baseline = nil)
      Gate.gate!(current, baseline)
    end

    # Re-bless the anchor (benchmark/baseline.json) from a results JSON.
    def bless(run)
      Gate.bless!(run)
    end

    # Stage-2 arbiter: paired alternation against a released Guest Binary
    # (a version reference or an explicit wasm path).
    def confirm(ref, pairs: Confirm::PAIRS)
      Confirm.confirm!(ref, pairs: pairs)
    end

    # Markdown head-vs-base comparison for the PR job summary, from two
    # results JSON paths.
    def report(current, baseline)
      Report.render(JSON.parse(File.read(current)), JSON.parse(File.read(baseline)))
    end
  end
end
