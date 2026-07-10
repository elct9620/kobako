# frozen_string_literal: true

require "test_helper"
require_relative "../../benchmark/support/confirm"

# Verdict math only — the IO shell (arm running, wasm resolution) is
# exercised manually via `rake bench:confirm`.
#
# Witness rationale: the thresholds encode the 2026-06-07 noise
# investigation — single pairs swing ±6-7% on this class of machine, so
# only direction-unanimous pairs with a mean past ±3% may be called.
class KobakoBenchConfirmTest < Minitest::Test
  def test_unanimous_slowdown_past_threshold_is_a_regression
    rows = Kobako::Bench::Confirm.judge(pairs(base: 100.0, currents: [94.0, 95.0, 93.0]))

    assert_equal :regression, rows.first.verdict,
                 "every pair slower with the mean past -3% through Confirm.judge must confirm a regression"
  end

  def test_mixed_directions_are_noise
    rows = Kobako::Bench::Confirm.judge(pairs(base: 100.0, currents: [93.0, 107.0, 94.0]))

    assert_equal :noise, rows.first.verdict,
                 "pairs disagreeing on direction through Confirm.judge must read as machine noise"
  end

  def test_unanimous_but_sub_threshold_mean_is_noise
    rows = Kobako::Bench::Confirm.judge(pairs(base: 100.0, currents: [99.0, 98.5, 99.2]))

    assert_equal :noise, rows.first.verdict,
                 "unanimous direction with the mean inside ±3% through Confirm.judge must read as machine noise"
  end

  def test_unanimous_speedup_past_threshold_is_an_improvement
    rows = Kobako::Bench::Confirm.judge(pairs(base: 100.0, currents: [105.0, 106.0, 107.0]))

    assert_equal :improvement, rows.first.verdict,
                 "every pair faster with the mean past +3% through Confirm.judge must read as an improvement"
  end

  def test_wide_pair_spread_is_unstable_even_when_unanimous
    rows = Kobako::Bench::Confirm.judge(pairs(base: 100.0, currents: [60.0, 95.0, 90.0]))

    assert_equal :unstable, rows.first.verdict,
                 "pairs spreading past ±20% through Confirm.judge must void the arbitration as unstable"
  end

  private

  # Build judge() samples: one label, a fixed baseline ips, and one
  # current ips per pair.
  def pairs(base:, currents:)
    currents.map { |current| [{ "4x-probe" => base }, { "4x-probe" => current }] }
  end
end
