# frozen_string_literal: true

require "test_helper"
require "tmpdir"

require_relative "../../benchmark/support/gate"

# Unit coverage for the release-gate runner ({SPEC.md Regression
# benchmarks}): path resolution defaulting to the committed anchor, and
# the {Gate.bless!} guards that refuse to overwrite the anchor from an
# absent or non-results source. The judgment itself lives in
# Kobako::Bench::Comparator and is covered separately.
class KobakoBenchGateTest < Minitest::Test
  Gate = Kobako::Bench::Gate

  def test_resolve_defaults_the_baseline_to_the_committed_anchor_path
    assert_equal Gate::ANCHOR_PATH, Gate.resolve("run.json", nil).last,
                 "with no explicit baseline the gate compares against the fixed anchor, not the previous run"
  end

  def test_resolve_returns_explicit_arguments_unchanged
    assert_equal ["run.json", "other.json"], Gate.resolve("run.json", "other.json")
  end

  # The suites {Gate.judge} subtracts from the comparison, so this has
  # to answer with a list on both branches — the quiet one included.
  def test_no_method_change_reports_an_empty_list_rather_than_nothing
    same = { "methods" => { "codec" => 2 } }

    assert_empty capture_io { assert_equal [], Gate.note_remethoded(same, same) }.first,
                 "two payloads agreeing on every method through Gate.note_remethoded must yield an " \
                 "empty list and print nothing"
  end

  def test_a_suite_measured_differently_than_the_anchor_is_named_and_returned
    changed = nil
    out = capture_io do
      changed = Gate.note_remethoded({ "methods" => { "codec" => 2 } }, { "methods" => {} })
    end.first

    assert_equal ["codec"], changed,
                 "a suite whose method moved since the anchor through Gate.note_remethoded must be " \
                 "returned, so the comparison can leave it out"
    assert_includes out, "codec", "the same suite must be named in the gate's output"
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
