# frozen_string_literal: true

require "minitest/autorun"

require_relative "report"

# Unit coverage for the head-vs-base PR report ({SPEC.md Regression
# benchmarks}): the per-row verdict (regression / improvement / within
# noise) and that the rendered Markdown carries the summary counts and a
# flagged regression row. Payloads are hand-built so each test states
# only the field it is about, reusing the gate's two metric shapes —
# +ips_row+ the host-row pair, +seconds_row+ a cold-path row carrying no
# gate metric.
class KobakoBenchReportTest < Minitest::Test
  Report = KobakoBench::Report

  def test_a_slowdown_past_the_floor_and_band_is_a_regression
    assert_equal :regression, Report.status_for(20.0, 5.0),
                 "a +20% slowdown clearing the 10% floor and a 5% band must read as a regression"
  end

  def test_a_speedup_clearing_the_band_is_an_improvement
    assert_equal :improvement, Report.status_for(-20.0, 5.0),
                 "a 20% speed-up past the noise band must read as an improvement"
  end

  def test_a_slowdown_under_the_floor_reads_as_within_noise
    assert_equal :stable, Report.status_for(5.0, 1.0),
                 "a +5% slowdown under the 10% floor must not be called a regression"
  end

  def test_a_move_inside_the_band_reads_as_within_noise
    assert_equal :stable, Report.status_for(8.0, 12.0),
                 "movement smaller than the noise band must read as within noise either direction"
  end

  def test_a_cold_path_row_without_a_gate_metric_is_left_out_of_the_comparison
    rows = Report.compare_rows(payload([seconds_row("c", 0.001)]), payload([seconds_row("c", 0.001)]), ["demo"])

    assert_empty rows, "one_shot / seconds rows carry no dispersion to gate on, so they are not compared"
  end

  def test_render_reports_the_counts_and_flags_a_regression_row
    current = payload([ips_row("h", 850.0, 5.0)], sha: "head123")
    baseline = payload([ips_row("h", 1000.0, 5.0)], sha: "base456")

    markdown = Report.render(current, baseline, suites: ["demo"])

    assert_includes markdown, "⚠️ 1 regressions",
                    "a -15% ips drop through Report.render must be counted as one regression in the summary"
    assert_includes markdown, "⚠️ regression",
                    "the regressed row through Report.render must carry the regression status"
    assert_includes markdown, "head `head123` vs base `base456`", "the report must name the two compared revisions"
  end

  private

  def ips_row(label, ips, deviation)
    { "label" => label, "ips" => ips, "ips_sd" => deviation }
  end

  def seconds_row(label, seconds)
    { "label" => label, "seconds" => seconds, "mode" => "one_shot" }
  end

  def payload(rows, sha: "abc1234")
    { "env" => { "git_sha" => sha, "ruby_version" => "3.4.7" }, "suites" => { "demo" => rows } }
  end
end
