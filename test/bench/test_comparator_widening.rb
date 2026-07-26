# frozen_string_literal: true

require "test_helper"

require_relative "../../benchmark/support/comparator"

# Unit coverage for the archive half of the release gate's noise band
# ({SPEC.md Regression benchmarks}). The anchor baseline moves only by a
# deliberate re-bless, but the archive the band reads grows whenever a run
# is committed — so an unbounded band would let an ordinary commit raise a
# row's bar, and a silent one would let it read as a pass the floor still
# governs. These pin the ceiling and the report that keeps it legible.
class KobakoBenchComparatorWideningTest < Minitest::Test
  Comparator = Kobako::Bench::Comparator

  def test_the_archive_band_stops_widening_at_its_ceiling
    # A move of 0.5 asks for 2 × 0.5 × 100 = 100%.
    assert_in_delta Comparator::MAX_ARCHIVE_BAND_PCT, Comparator.between_run_band(0.5), 0.001,
                    "an archive estimate past the ceiling must stop widening the bar — the archive " \
                    "grows by ordinary means, so an unbounded band turns a row off without a decision"
  end

  def test_a_row_the_archive_widens_is_reported_even_when_it_does_not_regress
    widened = widened_demo(ips_row("h", 995.0, 5.0), ips_row("h", 1000.0, 5.0),
                           history: { ["demo", "h", :ips] => 0.1 })

    assert_equal ["h"], widened.map(&:label),
                 "a gated row whose bar comes from the archive must be named on a clean pass too — " \
                 "a pass the floor no longer governs is looser than it reads"
    assert_operator widened.first.archive_pct, :>, widened.first.recorded_pct
    refute widened.first.capped, "a band below the ceiling must not report as capped"
  end

  def test_an_archive_band_under_the_floor_is_not_a_widening
    assert_empty widened_demo(ips_row("h", 995.0, 5.0), ips_row("h", 1000.0, 5.0),
                              history: { ["demo", "h", :ips] => 0.02 }),
                 "under the floor the bar is the floor either way, so a wider archive term there " \
                 "changes no verdict and must not crowd out the rows where it does"
  end

  def test_a_row_whose_own_run_is_noisier_than_the_archive_is_not_reported_as_widened
    assert_empty widened_demo(wall_row("r", 0.0001, 2.0e-5), wall_row("r", 0.0001, 2.0e-5),
                              history: { ["demo", "r", :wall_time] => 0.001 }),
                 "the archive only widens when it exceeds the run's own dispersion, so a quiet " \
                 "archive on a noisy row is not a widening"
  end

  def test_a_capped_row_reports_that_it_hit_the_ceiling
    widened = widened_demo(ips_row("h", 995.0, 5.0), ips_row("h", 1000.0, 5.0),
                           history: { ["demo", "h", :ips] => 0.9 })

    assert widened.first.capped,
           "a row the ceiling clamped must say so, so the reader sees the archive asked for more"
  end

  private

  # Archive-widened rows for a single-row current run against a single-row
  # anchor under the synthetic "demo" suite.
  def widened_demo(current_row, base_row, history: {})
    Comparator.archive_widened(payload([current_row]), payload([base_row]),
                               suites: ["demo"], history: history)
  end

  def wall_row(label, wall, deviation)
    { "label" => label, "wall_time" => wall, "wall_time_sd" => deviation }
  end

  def ips_row(label, ips, deviation)
    { "label" => label, "ips" => ips, "ips_sd" => deviation }
  end

  def payload(rows)
    { "suites" => { "demo" => rows } }
  end
end
