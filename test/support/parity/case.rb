# frozen_string_literal: true

module Parity
  # Base class for the differential parity families under
  # test/parity/: one Scenario, two executors, one equality. The
  # normalized observables must match field-for-field; +message+ and
  # raw usage numbers are diagnostic-only (host-generated wording and
  # timing legitimately differ), so normalization reduces usage to
  # presence and drops messages before comparing.
  class Case < Minitest::Test
    include E2eGuestHelper

    def assert_parity(scenario)
      require_cargo_oracle!(rust_executor.oracle)
      ruby = RubyExecutor.new(E2eGuestHelper::REAL_WASM).execute(scenario)
      rust = rust_executor.execute(scenario)
      assert_equal normalize(ruby), normalize(rust),
                   "scenario #{scenario.name} through " \
                   "Kobako::Sandbox and the kobako SDK must observe identically"
    end

    private

    def rust_executor
      @rust_executor ||= RustExecutor.new(E2eGuestHelper::REAL_WASM)
    end

    def normalize(observables)
      observables.map do |observable|
        observable.except("message", "usage")
                  .merge("usage_present" => !observable["usage"].nil?)
      end
    end
  end
end
