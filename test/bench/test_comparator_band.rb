# frozen_string_literal: true

require "test_helper"

require_relative "../../benchmark/support/comparator"

# Unit coverage for the within-run half of the release gate's noise band
# ({SPEC.md Regression benchmarks}). The figure the gate compares is a
# median, so the band has to be the uncertainty of that median rather than
# the spread of one sample it was reduced from — these pin the scaling, the
# count each metric reads it from, and the asymmetric pair a run forms
# against an anchor captured before the count existed.
class KobakoBenchComparatorBandTest < Minitest::Test
  Comparator = Kobako::Bench::Comparator

  def test_noise_band_combines_both_runs_standard_errors_in_quadrature
    row = ips_row("r", 100.0, 2.0)

    # One observation each, so the deviation stands whole:
    # 2 × √(0.02² + 0.02²) × 100 ≈ 5.657%.
    assert_in_delta 5.657, Comparator.noise_band(row, row, :ips), 0.01,
                    "two rows recording no sample count through Comparator.noise_band must combine " \
                    "their deviations in quadrature, keeping each whole"
  end

  def test_a_median_over_many_samples_narrows_the_band_its_deviation_alone_would_set
    many = ips_row("r", 100.0, 2.0).merge("cycles" => 25)

    # 25 samples scale the deviation by 1.2533/√25, a quarter of what the
    # same spread sets on a single observation.
    assert_in_delta 1.418, Comparator.noise_band(many, many, :ips), 0.01,
                    "a row recording 25 samples through Comparator.noise_band must gate on the " \
                    "standard error of its median, not on the spread of one sample"
  end

  def test_a_wall_time_row_scales_its_deviation_by_the_sample_count_it_recorded
    sampled = wall_row("r", 0.0001, 1.0e-5).merge("wall_time_samples" => 100)

    assert_in_delta 3.545, Comparator.noise_band(sampled, sampled, :wall_time), 0.01,
                    "a wall_time row carrying its sample count through Comparator.noise_band must " \
                    "scale by that count, the way an ips row scales by its cycles"
  end

  # The pair a run-vs-anchor comparison actually forms until the next
  # re-bless: the run counts its samples, the anchor predates the count.
  def test_a_sampled_row_against_an_uncounted_one_carries_both_halves_of_the_quadrature
    sampled = wall_row("r", 0.0001, 1.0e-5).merge("wall_time_samples" => 200)
    uncounted = wall_row("r", 0.0001, 1.0e-5)

    # 2 × √(0.00886² + 0.1²) × 100 — the uncounted half dominates and survives.
    assert_in_delta 20.078, Comparator.noise_band(sampled, uncounted, :wall_time), 0.01,
                    "a sampled row compared against one recording no count through " \
                    "Comparator.noise_band must scale each side by its own count, not by either alone"
  end

  private

  def ips_row(label, ips, deviation)
    { "label" => label, "ips" => ips, "ips_sd" => deviation }
  end

  def wall_row(label, wall, deviation)
    { "label" => label, "wall_time" => wall, "wall_time_sd" => deviation }
  end
end
