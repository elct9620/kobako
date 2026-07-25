# frozen_string_literal: true

require "test_helper"
require "json"
require "tmpdir"

require_relative "../../benchmark/support/history"

# Unit coverage for the between-run dispersion estimate. The load-bearing
# property is robustness to a genuine step: the archive spans months of
# real optimisations, so an estimate that read those as noise would widen
# the band on exactly the rows that measure cleanly and blind the gate.
class KobakoBenchHistoryTest < Minitest::Test
  History = Kobako::Bench::History

  def test_a_steadily_moving_row_reports_its_typical_move
    moves = dispersion_of([100.0, 110.0, 100.0, 110.0, 100.0, 110.0])

    assert_in_delta 0.1, moves.fetch(%w[s r].push(:ips)), 0.01,
                    "a row that moves about a tenth between consecutive runs must report that " \
                    "move, so its band widens to the transient the archive actually shows"
  end

  def test_a_single_step_change_does_not_inflate_the_estimate
    moves = dispersion_of([100.0, 100.0, 100.0, 150.0, 150.0, 150.0])

    assert_in_delta 0.0, moves.fetch(%w[s r].push(:ips)), 0.001,
                    "an accepted one-time optimisation must leave the estimate at the quiet level " \
                    "around it — a step is one outlier among the moves, not the row's noise"
  end

  # The old runs outnumber the window deliberately: the median is robust
  # to a noisy minority on its own, so only an old NOISY MAJORITY can tell
  # a bounded lookback apart from an unbounded one.
  def test_runs_older_than_the_window_do_not_reach_the_estimate
    era = History::WINDOW * 2
    moves = dispersion_of(Array.new(era) { |i| i.even? ? 100.0 : 200.0 } + Array.new(era, 100.0))

    assert_in_delta 0.0, moves.fetch(%w[s r].push(:ips)), 0.001,
                    "a row that has settled must be estimated from the recent runs alone — an " \
                    "unbounded lookback would hold its band open on noise it no longer shows"
  end

  def test_a_row_seen_in_too_few_runs_reports_nothing
    moves = dispersion_of([100.0, 110.0, 100.0])

    assert_nil moves[%w[s r].push(:ips)],
               "a row with fewer consecutive moves than the minimum must report no estimate, so " \
               "the gate falls back to the within-run band instead of trusting two points"
  end

  def test_a_row_is_keyed_by_every_metric_it_carries
    moves = Dir.mktmpdir do |dir|
      6.times { |i| write_run(dir, i, { "label" => "r", "ips" => 100.0, "wall_time" => 0.001 }) }
      History.dispersion(File.join(dir, "*.json"))
    end

    assert_equal [%w[s r].push(:ips), %w[s r].push(:wall_time)].sort, moves.keys.sort,
                 "a row carrying both metrics must be estimated under each, so the gate finds the " \
                 "key for whichever metric it chose to gate that row on"
  end

  private

  # Estimate over one synthetic ips series, one archived run per value.
  def dispersion_of(values)
    Dir.mktmpdir do |dir|
      values.each_with_index { |ips, i| write_run(dir, i, { "label" => "r", "ips" => ips }) }
      History.dispersion(File.join(dir, "*.json"))
    end
  end

  # One archived run carrying +row+ in suite "s". The captured_at stamp
  # orders the series; the filename deliberately does not.
  def write_run(dir, index, row)
    payload = { "env" => { "captured_at" => format("2026-01-%02dT00:00:00Z", index + 1) },
                "suites" => { "s" => [row] } }
    File.write(File.join(dir, "run-#{9 - index}.json"), JSON.generate(payload))
  end
end
