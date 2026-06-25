# frozen_string_literal: true

module Kobako
  module Bench
    # The Guest Binary a probe runs against. +bench:confirm+ sets
    # KOBAKO_BENCH_WASM to point an arm at a baseline without touching
    # data/kobako.wasm; with it unset a probe keeps its own default (the
    # gem-bundled binary, or a variant it names explicitly).
    module Guest
      ENV_KEY = "KOBAKO_BENCH_WASM"

      module_function

      # The KOBAKO_BENCH_WASM override when set, else +fallback+ — which a
      # probe passes straight to +Kobako::Sandbox.new(wasm_path:)+, where
      # +nil+ selects the gem-bundled default.
      def path(fallback = nil)
        ENV.fetch(ENV_KEY, nil) || fallback
      end
    end
  end
end
