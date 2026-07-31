# frozen_string_literal: true

require "test_helper"
require "json"
require "tmpdir"

require_relative "../../benchmark/support/runner"

# Unit coverage for the capture stamp a results file carries. Every probe
# of one round merges into a single +<date>-<sha>.json+, so the stamp has
# to follow the round that wrote the newest suites: a re-run on the same
# day at the same sha would otherwise report its measurements under the
# machine state of the round before it, and the file would read as a
# clean capture while saying so about the wrong one.
class KobakoBenchResultsTest < Minitest::Test
  Runner = Kobako::Bench::Runner
  Results = Kobako::Bench::Results

  PRIOR_SHA = "deadbee"
  PRIOR_STAMP = "2020-01-01T00:00:00Z"

  def setup
    @dir = Dir.mktmpdir
    @lock = File.join(@dir, ".bench.lock")
  end

  def teardown
    FileUtils.remove_entry(@dir)
  end

  # Keeping the older stamp would describe this round's measurements by
  # the machine state of the round before it.
  def test_a_write_from_a_later_round_restamps_the_file
    seed_prior_round
    lock_at(Time.now)

    refute_equal PRIOR_SHA, env_of(write_suite("second"))["git_sha"],
                 "a file stamped by an earlier round, written through Runner#write! under a newer " \
                 "round marker, must carry a re-stamped env"
  end

  # The complement: within one round every probe has to report the same
  # capture, not the machine state of whichever finished last.
  def test_a_write_within_the_same_round_keeps_that_round_s_stamp
    seed_prior_round
    lock_at(Time.utc(2019))

    assert_equal PRIOR_SHA, env_of(write_suite("second"))["git_sha"],
                 "a file written through Runner#write! under the round marker it was already stamped " \
                 "in must keep that stamp"
  end

  # A probe driven directly holds no lock and is a round of its own, so
  # inheriting a stamp would attribute it to a round it never belonged to.
  def test_a_probe_driven_without_a_round_marker_restamps_the_file
    seed_prior_round

    refute_equal PRIOR_SHA, env_of(write_suite("second"))["git_sha"],
                 "a file written through Runner#write! with no round marker present must carry a " \
                 "re-stamped env"
  end

  # The stamp answers which round a capture is from, not which suites the
  # file may keep.
  def test_a_later_round_keeps_the_suites_already_in_the_file
    seed_prior_round
    lock_at(Time.now)

    assert_equal %w[first second], JSON.parse(File.read(write_suite("second")))["suites"].keys.sort,
                 "a re-stamping write through Runner#write! must keep every suite already merged " \
                 "into the file"
  end

  # A capture says which measurement method produced it, so a later
  # reader can tell the runs that are comparable from the ones that are
  # only adjacent.
  def test_a_write_stamps_the_suite_s_measurement_method
    methods = JSON.parse(File.read(write_suite("codec")))["methods"]

    assert_equal Kobako::Bench::METHOD_VERSIONS.fetch("codec"), methods["codec"],
                 "a suite carrying a method version through Runner#write! must record it beside " \
                 "its rows"
  end

  # A version keyed to a name no suite answers to is a bump that never
  # happens — the archive it was meant to retire stays in the estimate.
  def test_every_declared_method_version_names_a_suite_that_exists
    known = Kobako::Bench::RELEASE_BENCHES.map { |path| File.basename(path, ".rb") } +
            Kobako::Bench::SWEEP_TASKS

    assert_empty Kobako::Bench::METHOD_VERSIONS.keys - known,
                 "every key in METHOD_VERSIONS must name a suite the roster runs, or the bump it " \
                 "records is silently a no-op"
  end

  def test_a_merged_write_keeps_the_method_of_a_suite_already_in_the_file
    seed_prior_round
    lock_at(Time.now)

    assert_equal %w[codec first], JSON.parse(File.read(write_suite("codec")))["methods"].keys.sort,
                 "a re-stamping write through Runner#write! must keep the method of every suite " \
                 "already merged into the file, the way it keeps their rows"
  end

  def test_a_suite_that_has_never_changed_method_records_the_first_version
    methods = JSON.parse(File.read(write_suite("never-remethoded")))["methods"]

    assert_equal 1, methods["never-remethoded"],
                 "a suite absent from METHOD_VERSIONS through Runner#write! must record version 1, " \
                 "so every capture carries a complete map rather than a partial one"
  end

  private

  # Write one suite, then age its stamp so a later write has something
  # recognisably foreign to either keep or replace.
  def seed_prior_round
    path = write_suite("first")
    payload = JSON.parse(File.read(path))
    payload["env"].merge!("git_sha" => PRIOR_SHA, "captured_at" => PRIOR_STAMP)
    File.write(path, JSON.generate(payload))
    path
  end

  # Drive one suite through the whole write path with the results
  # directory pointed at the tmp dir, restoring the override afterwards.
  def write_suite(name)
    prior = ENV.fetch(Results::RESULTS_DIR_ENV, nil)
    ENV[Results::RESULTS_DIR_ENV] = @dir
    runner = Runner.new(name, lock: @lock)
    runner.results << { label: "a", ips: 1.0 }
    runner.write!
  ensure
    ENV[Results::RESULTS_DIR_ENV] = prior
  end

  # Stand a round marker at +time+, the moment that round started.
  def lock_at(time)
    File.write(@lock, "#{Process.pid}\n")
    File.utime(time, time, @lock)
  end

  def env_of(path)
    JSON.parse(File.read(path))["env"]
  end
end
