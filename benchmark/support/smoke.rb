# frozen_string_literal: true

module Kobako
  module Bench
    # Smoke surface of {Runner}, mixed in beside {OneShot} so the
    # measurement machinery and the wiring check live in separate files.
    # A smoke pass answers whether a probe still loads and its case
    # bodies still run — the breakage a probe suffers when the API it
    # drives is renamed or removed — without paying for measurement.
    # Keeping the seam here is what lets +gate:bench:smoke+ cover every
    # probe while no probe carries a smoke branch of its own. Relies on
    # the including class for +@results+.
    module Smoke
      # Environment flag +gate:bench:smoke+ sets on each probe it drives.
      ENV_NAME = "KOBAKO_BENCH_SMOKE"

      # True when this runner is smoking rather than measuring.
      def smoke?
        @smoke
      end

      private

      # Run the case body once and record the label. The row carries no
      # measurement on purpose — a measured-looking row would invite a
      # reader to compare it against a real run.
      def smoke_case(label)
        yield
        @results << { label: label, mode: "smoke" }
      end
    end
  end
end
