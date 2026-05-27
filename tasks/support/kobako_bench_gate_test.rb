# frozen_string_literal: true

require "minitest/autorun"
require "tmpdir"

require_relative "kobako_bench_gate"

# Unit coverage for the release-gate runner ({SPEC.md Regression
# benchmarks}): path resolution defaulting to the committed anchor, and
# the {Gate.bless!} guards that refuse to overwrite the anchor from an
# absent or non-results source. The judgment itself lives in
# KobakoBench::Comparator and is covered separately.
class KobakoBenchGateTest < Minitest::Test
  Gate = KobakoBench::Gate

  def test_resolve_defaults_the_baseline_to_the_committed_anchor_path
    assert_equal Gate::ANCHOR_PATH, Gate.resolve("run.json", nil).last,
                 "with no explicit baseline the gate compares against the fixed anchor, not the previous run"
  end

  def test_resolve_returns_explicit_arguments_unchanged
    assert_equal ["run.json", "other.json"], Gate.resolve("run.json", "other.json")
  end

  def test_bless_refuses_a_nil_source
    assert_raises(RuntimeError, "bless with no source must refuse rather than touch the anchor") do
      Gate.bless!(nil)
    end
  end

  def test_bless_refuses_a_missing_source
    assert_raises(RuntimeError, "bless from a non-existent path must refuse rather than touch the anchor") do
      Gate.bless!("/no/such/run.json")
    end
  end

  def test_bless_refuses_a_source_that_is_not_a_results_payload
    Dir.mktmpdir do |dir|
      path = File.join(dir, "bad.json")
      File.write(path, "not benchmark json")

      assert_raises(RuntimeError,
                    "bless from a non-results file must refuse so a malformed anchor cannot crash the next gate") do
        Gate.bless!(path)
      end
    end
  end
end
