# frozen_string_literal: true

module Parity
  # Drives the `crates/kobako-parity` runner — the Rust half of the
  # differential harness — over the CargoOracle framed protocol: one
  # scenario JSON per frame in, the raw observables array back.
  class RustExecutor
    CRATE_DIR = File.expand_path("../../../crates/kobako-parity", __dir__)

    def initialize(wasm_path)
      @wasm_path = wasm_path
      @oracle = CargoOracle.new(crate_dir: CRATE_DIR, bin_name: "parity_runner")
    end

    # Memoised release build of the runner; callers skip on
    # +:no_cargo+ and flunk on +:build_failed+ (CargoOracle contract).
    def ensure_built
      @oracle.ensure_built
    end

    def execute(scenario)
      @oracle.open do |channel|
        channel.send_frame(JSON.generate(scenario.to_payload(@wasm_path)))
        body, error = channel.read_frame
        raise "parity runner rejected the scenario: #{body}" if error

        JSON.parse(body)
      end
    end
  end
end
