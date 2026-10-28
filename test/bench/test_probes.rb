# frozen_string_literal: true

require "test_helper"
require "open3"

require_relative "../../benchmark/support/roster"
require_relative "../../benchmark/support/smoke"

# Every probe in the smoke roster, run once under the smoke seam. Nothing
# else runs the probes, so a renamed or removed API rots one silently;
# dispatch_glue.rb drove a deleted type for twenty commits. Each runs
# whole in its own process, as +rake bench+ would, and answers wiring
# rather than cost.
class KobakoBenchProbesTest < Minitest::Test
  include GuestGuard

  PROBES_DIR = File.join(Kobako::Bench::Paths::ROOT, "benchmark", "")

  Kobako::Bench::SMOKE_BENCHES.each do |probe|
    name = probe.delete_prefix(PROBES_DIR).delete_suffix(".rb")

    define_method("test_#{name.tr("/", "_")}_runs_every_case_under_smoke") do
      require_guest_binary!(TestPaths.data("kobako.wasm"), build: "rake wasm:build")
      output, status = Open3.capture2e({ Kobako::Bench::Smoke::ENV_NAME => "1" }, RbConfig.ruby, probe,
                                       chdir: TestPaths::ROOT)

      assert_predicate status, :success?,
                       "benchmark/#{name}.rb under KOBAKO_BENCH_SMOKE must load and run every case body, " \
                       "so the API it drives still exists:\n#{output.lines.last(15).join}"
    end
  end
end
