# frozen_string_literal: true

require "test_helper"

require_relative "../../benchmark/support/roster"

# Unit coverage for the benchmark rosters. The load-bearing one is
# classification: every probe on disk is either smoked by the release
# gate or carries a written reason for staying out, so a probe added
# later cannot slip past the gate unnoticed — which is exactly how
# dispatch_glue.rb went twenty commits driving a deleted API.
class KobakoBenchRosterTest < Minitest::Test
  Bench = Kobako::Bench

  def test_every_probe_on_disk_is_either_smoked_or_excluded_with_a_reason
    classified = Bench::SMOKE_BENCHES + Bench::SMOKE_EXCLUSIONS.keys.map { |name| Bench::Paths.probe(name) }

    assert_equal [], probes_on_disk - classified,
                 "a probe added under benchmark/ must be added to SMOKE_BENCHES or to " \
                 "SMOKE_EXCLUSIONS with its reason — an unclassified probe is one the gate never runs"
  end

  def test_every_gated_benchmark_is_also_smoked
    assert_equal [], Bench::RELEASE_BENCHES - Bench::SMOKE_BENCHES,
                 "a benchmark promoted into the release roster must be smoked too, so gating a " \
                 "probe never costs it the cheaper wiring check"
  end

  def test_the_whole_round_sweep_does_not_re_run_a_gated_suite
    gated = Bench::RELEASE_BENCHES.map { |path| File.basename(path, ".rb") }

    assert_equal [], Bench::SWEEP_TASKS & gated,
                 "the whole-round sweep must not name a suite the gated set already runs — a second " \
                 "pass would overwrite the archived rows with ones captured at a different moment"
  end

  def test_no_probe_is_both_smoked_and_excluded
    excluded = Bench::SMOKE_EXCLUSIONS.keys.map { |name| Bench::Paths.probe(name) }

    assert_equal [], Bench::SMOKE_BENCHES & excluded,
                 "a probe listed as a smoke exclusion must not also be smoked — the two rosters " \
                 "state opposite intents and the gate would report both"
  end

  private

  def probes_on_disk
    Dir[Bench::Paths::PROBE_GLOB]
  end
end
