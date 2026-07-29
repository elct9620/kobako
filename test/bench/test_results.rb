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

  def test_a_write_from_a_later_round_restamps_the_file
    seed_prior_round
    lock_at(Time.now)

    refute_equal PRIOR_SHA, env_of(write_suite("second"))["git_sha"],
                 "a suite written under a round marker newer than the recorded stamp must re-stamp env — " \
                 "keeping it would describe this round's measurements by the previous round's machine state"
  end

  def test_a_write_within_the_same_round_keeps_that_round_s_stamp
    seed_prior_round
    lock_at(Time.utc(2019))

    assert_equal PRIOR_SHA, env_of(write_suite("second"))["git_sha"],
                 "a suite written under the round marker its file was already stamped in must keep that stamp, " \
                 "so one round's probes report one capture rather than the last one to finish"
  end

  def test_a_probe_driven_without_a_round_marker_restamps_the_file
    seed_prior_round

    refute_equal PRIOR_SHA, env_of(write_suite("second"))["git_sha"],
                 "a probe run with no round marker must re-stamp env — a direct run is a capture of its own, " \
                 "and inheriting a stamp would attribute it to a round it never belonged to"
  end

  def test_a_later_round_keeps_the_suites_already_in_the_file
    seed_prior_round
    lock_at(Time.now)

    assert_equal %w[first second], JSON.parse(File.read(write_suite("second")))["suites"].keys.sort,
                 "re-stamping must not drop the suites already merged into the file — the stamp answers " \
                 "which round a capture is from, not which suites it may keep"
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
