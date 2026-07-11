# frozen_string_literal: true

module Parity
  # One declarative parity scenario — the pure-data description both
  # executors interpret against the same Guest Binary. Stub behaviors
  # (+"echo"+ / +"value"+ / +"raise"+), invocation verbs (+"eval"+ /
  # +"run"+ / +"late_bind"+), and preload kinds (+"source"+ /
  # +"bytecode"+, the latter carrying RITE bytes as hex) form closed
  # sets that grow append-only with the corpus; +undefined+ /
  # +argument+ faults arise from the scenario's shape (a method the
  # stub lacks), never from a stub declaration, so both dispatchers
  # must derive them from the same conditions.
  SCENARIO_DEFAULTS = { anchors: [], options: {}, services: [], preloads: [] }.freeze

  Scenario = Data.define(:name, :anchors, :options, :services, :preloads, :invocations) do
    def initialize(name:, invocations:, **rest)
      super(name:, invocations:, **SCENARIO_DEFAULTS.merge(rest))
    end

    # The JSON-ready payload the Rust runner consumes; the Ruby
    # executor reads the Scenario directly.
    def to_payload(wasm_path)
      {
        "wasm_path" => wasm_path,
        "options" => options,
        "services" => services,
        "preloads" => preloads,
        "invocations" => invocations
      }
    end
  end
end
