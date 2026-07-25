# frozen_string_literal: true

require "test_helper"
require "tmpdir"

require_relative "../../benchmark/support/runner"

# Unit coverage for the smoke seam +gate:bench:smoke+ drives every probe
# through. The seam has to hold two lines at once: under smoke a case
# body runs exactly once, and outside smoke the measurement loop is
# untouched — so a leaked flag cannot silently turn the release
# benchmarks into a single unmeasured iteration.
class KobakoBenchSmokeTest < Minitest::Test
  Runner = Kobako::Bench::Runner
  Smoke = Kobako::Bench::Smoke

  def test_a_case_under_smoke_runs_its_body_exactly_once
    calls = 0
    smoking { Runner.new("t").case("a") { calls += 1 } }

    assert_equal 1, calls,
                 "a probe case under KOBAKO_BENCH_SMOKE must execute its body exactly once — " \
                 "the gate asks whether the body runs, not what it costs"
  end

  def test_a_case_outside_smoke_still_iterates
    calls = 0
    silently { Runner.new("t", time: 0.01, warmup: 0).case("a") { calls += 1 } }

    assert_operator calls, :>, 1,
                    "a probe case with no KOBAKO_BENCH_SMOKE must still iterate, so the seam " \
                    "cannot silently reduce a measured benchmark to one unmeasured pass"
  end

  def test_a_smoked_case_records_a_row_for_later_annotation
    runner = smoking { Runner.new("t").tap { |r| r.case("a") { nil } } }

    assert_equal ["a"], runner.results.map { |row| row[:label] },
                 "a smoked case must still record its row so a probe's follow-on " \
                 "annotate_usage! has a target instead of raising on an empty result set"
  end

  def test_case_with_usage_under_smoke_does_not_drive_the_sampling_loop
    calls = 0
    smoking { Runner.new("t").case_with_usage("a") { calls += 1 } }

    assert_equal 1, calls,
                 "case_with_usage under KOBAKO_BENCH_SMOKE must skip the usage sampling loop — " \
                 "its eleven-sample floor would cost more than the smoke pass it rides on"
  end

  def test_a_smoked_run_writes_no_results_file
    written = Dir.mktmpdir do |dir|
      smoked_write_into(dir)
      Dir.children(dir)
    end

    assert_equal [], written,
                 "a smoked run must write no results file at all — a measurement-free suite merged " \
                 "into benchmark/results/ would silently replace a real capture's rows"
  end

  private

  # Drive one smoked case through the whole write path with the results
  # directory pointed at +dir+, restoring the override afterwards.
  def smoked_write_into(dir)
    prior = ENV.fetch(Runner::RESULTS_DIR_ENV, nil)
    ENV[Runner::RESULTS_DIR_ENV] = dir
    smoking { Runner.new("t").tap { |runner| runner.case("a") { nil } }.write! }
  ensure
    ENV[Runner::RESULTS_DIR_ENV] = prior
  end

  # Run the block with the smoke flag set, restoring the prior value so
  # one test cannot leak the mode into the next.
  def smoking(&block)
    prior = ENV.fetch(Smoke::ENV_NAME, nil)
    ENV[Smoke::ENV_NAME] = "1"
    silently(&block)
  ensure
    ENV[Smoke::ENV_NAME] = prior
  end

  # Swallow the per-case progress line the measured path prints.
  def silently
    original = $stdout
    $stdout = StringIO.new
    yield
  ensure
    $stdout = original
  end
end
