# frozen_string_literal: true

require "minitest/autorun"

require_relative "kobako_bench_gate"

# Unit coverage for the release-gate comparator
# ({SPEC.md Regression benchmarks}). Drives the pure decision surface —
# regression direction, the quadrature noise band, anchor-relative
# flagging, and anchor coverage — against hand-built result payloads, so
# each test states only the field it is about. The payload builders at
# the bottom carry the two metric shapes the gate reads: +wall_row+ the
# +wall_time+ pair sandbox-driven cases gate on, +ips_row+ the +ips+ pair
# pure-host rows gate on, and +seconds_row+ a cold-path row the gate
# skips because it carries no dispersion.
class KobakoBenchGateTest < Minitest::Test
  Gate = KobakoBench::Gate

  def test_wall_time_regression_is_positive_when_the_budget_rises
    assert_in_delta 20.0, Gate.regression_pct(:wall_time, 100.0, 120.0), 0.001
  end

  def test_ips_regression_is_positive_when_throughput_drops
    assert_in_delta 15.0, Gate.regression_pct(:ips, 1000.0, 850.0), 0.001
  end

  def test_an_improvement_yields_a_negative_regression_so_the_floor_rejects_it
    assert_operator Gate.regression_pct(:ips, 1000.0, 1100.0), :<, 0
  end

  def test_noise_band_combines_both_runs_coefficients_of_variation_in_quadrature
    # cv 2% on each run → 2 × √(0.02² + 0.02²) × 100 ≈ 5.657%.
    assert_in_delta 5.657, Gate.noise_band(100.0, 2.0, 100.0, 2.0), 0.01
  end

  def test_flags_a_wall_time_regression_that_clears_the_floor_and_a_tight_band
    findings = compare_demo(wall_row("r", 0.00012, 1.0e-7), wall_row("r", 0.0001, 1.0e-7))

    assert_equal 1, findings.size, "a +20% wall_time rise over a near-zero band must be flagged"
    assert_equal :wall_time, findings.first.metric
    assert_in_delta 20.0, findings.first.delta_pct, 0.5
  end

  def test_ignores_a_wall_time_regression_that_stays_within_the_floor
    findings = compare_demo(wall_row("r", 0.000105, 1.0e-7), wall_row("r", 0.0001, 1.0e-7))

    assert_empty findings, "a +5% rise under the +10% floor must not be flagged"
  end

  def test_suppresses_a_regression_that_clears_the_floor_but_not_the_noise_band
    findings = compare_demo(wall_row("r", 0.00012, 1.2e-5), wall_row("r", 0.0001, 1.0e-5))

    assert_empty findings, "a +20% rise inside a ~28% noise band must be suppressed as noise"
  end

  def test_flags_a_pure_host_row_on_its_ips_drop
    findings = compare_demo(ips_row("h", 850.0, 5.0), ips_row("h", 1000.0, 5.0))

    assert_equal :ips, findings.first&.metric, "a -15% ips drop on a host row must be flagged on ips"
  end

  def test_flags_a_gated_case_present_in_the_run_but_absent_from_the_anchor
    base = payload([wall_row("a", 0.0001, 1.0e-7)])
    current = payload([wall_row("a", 0.0001, 1.0e-7), wall_row("b", 0.0001, 1.0e-7)])

    missing = Gate.unanchored(current, base, suites: ["demo"])

    assert_equal ["b"], missing.map(&:label), "a new gated case with no anchor must fail, not pass silently"
  end

  def test_does_not_flag_a_cold_path_row_that_carries_no_gate_metric
    base = payload([wall_row("a", 0.0001, 1.0e-7)])
    current = payload([wall_row("a", 0.0001, 1.0e-7), seconds_row("c", 0.001)])

    assert_empty Gate.unanchored(current, base, suites: ["demo"]),
                 "one_shot / seconds rows carry no dispersion and are not gated, so absence is not a failure"
  end

  def test_resolve_defaults_the_baseline_to_the_committed_anchor_path
    assert_equal Gate::ANCHOR_PATH, Gate.resolve("run.json", nil).last,
                 "with no explicit baseline the gate compares against the fixed anchor, not the previous run"
  end

  def test_resolve_returns_explicit_arguments_unchanged
    assert_equal ["run.json", "other.json"], Gate.resolve("run.json", "other.json")
  end

  private

  # Compare a single-row current run against a single-row anchor under
  # the synthetic "demo" suite.
  def compare_demo(current_row, base_row)
    Gate.compare(payload([current_row]), payload([base_row]), suites: ["demo"])
  end

  def wall_row(label, wall, deviation)
    { "label" => label, "wall_time" => wall, "wall_time_sd" => deviation,
      "ips" => 1.0 / wall, "memory_peak" => 0 }
  end

  def ips_row(label, ips, deviation)
    { "label" => label, "ips" => ips, "ips_sd" => deviation }
  end

  def seconds_row(label, seconds)
    { "label" => label, "seconds" => seconds, "mode" => "one_shot" }
  end

  def payload(rows)
    { "suites" => { "demo" => rows } }
  end
end
