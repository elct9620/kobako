# frozen_string_literal: true

module Kobako
  module Bench
    # Single source of truth for repository-relative paths shared by the
    # benchmark probes and the rake-side gate/confirm tooling. Centralising
    # them here means a directory move re-points one file instead of every
    # consumer hand-tuning its own +__dir__+ depth.
    module Paths
      ROOT = File.expand_path("../..", __dir__)
      DATA_WASM = File.join(ROOT, "data", "kobako.wasm")
      RESULTS_DIR = File.join(ROOT, "benchmark", "results")
      RESULTS_GLOB = File.join(RESULTS_DIR, "*.json")
      BASELINE_ANCHOR = File.join(ROOT, "benchmark", "baseline.json")
      # The compiled native ext, under whichever suffix the platform
      # gives it. A probe cannot even load without it.
      NATIVE_EXT_GLOB = File.join(ROOT, "lib", "kobako", "kobako.{bundle,so}")
      # Run-in-progress marker the Stop-hook gate reads to defer while a
      # benchmark measures; the bash guard hardcodes the same tmp path.
      LOCK = File.join(ROOT, "tmp", ".bench.lock")

      module_function

      # Absolute path to a probe script under +benchmark/+, named without
      # its extension (e.g. +"mruby_eval"+).
      def probe(name)
        File.join(ROOT, "benchmark", "#{name}.rb")
      end

      # Absolute path to a Guest Binary variant under +data/+, named by its
      # feature suffix (e.g. +"regexp-unicode"+ for +kobako+regexp-unicode.wasm+).
      def variant_wasm(name)
        File.join(ROOT, "data", "kobako+#{name}.wasm")
      end

      # Every probe script under +dir+ at any depth, and only those:
      # +support/+ holds the tooling the probes drive rather than probes.
      # Reaching any depth is what lets the roster's classification check
      # see a probe a later reorganization groups into a directory of its
      # own, instead of leaving a blind spot where one would land.
      def probes(dir = File.join(ROOT, "benchmark"))
        Dir[File.join(dir, "**", "*.rb")].reject { |path| path.start_with?(File.join(dir, "support", "")) }
      end

      # Absolute path to a committed fixture under +test/fixtures/+. The
      # wasm fixtures live in one place whoever drives them, so a probe
      # standing a guest in declares the reach here rather than counting
      # directories of its own.
      def fixture(name)
        File.join(ROOT, "test", "fixtures", name)
      end
    end
  end
end
