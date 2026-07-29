# frozen_string_literal: true

require "fileutils"
require "json"
require "time"

require_relative "env"
require_relative "paths"

module Kobako
  module Bench
    # Results-file surface of {Runner}, mixed in beside {OneShot} and
    # {Smoke} so the measurement machinery and the on-disk capture live in
    # separate files. Owns where a run is written, how the suites of one
    # round share that file, and which round's machine state it reports.
    # Relies on the including class for +@suite+, +@results+, +@lock+, and
    # +#smoke?+.
    module Results
      # +bench:confirm+ points each arm's output at a throwaway directory so
      # the paired runs never collide with a real benchmark/results file.
      RESULTS_DIR_ENV = "KOBAKO_BENCH_RESULTS_DIR"

      # Persist the collected results to
      # +benchmark/results/<date>-<sha>.json+. Returns the absolute path.
      # Existing files for the same +(date, sha)+ pair are merged so
      # multiple +Runner+ instances within one round share a single output
      # file; a merge from a later round re-stamps +env+, so the machine
      # state a file reports is always the one its newest suites were
      # captured under. A smoke pass writes nothing: its rows carry no
      # measurement, and merging them would replace a real capture's suite
      # under the same +(date, sha)+ name.
      def write!
        return "#{@suite}: smoked, not measured — no results written" if smoke?

        FileUtils.mkdir_p(results_dir)
        path = result_path
        payload = load_payload(path)
        payload["suites"][@suite] = @results.map { |r| r.transform_keys(&:to_s) }
        File.write(path, JSON.pretty_generate(payload))
        path
      end

      private

      def result_path
        env = Env.snapshot
        date = Time.now.utc.strftime("%Y-%m-%d")
        File.join(results_dir, "#{date}-#{env[:git_sha]}.json")
      end

      # The KOBAKO_BENCH_RESULTS_DIR override when set, else the committed
      # +benchmark/results+ directory.
      def results_dir
        ENV.fetch(RESULTS_DIR_ENV, nil) || Paths::RESULTS_DIR
      end

      def load_payload(path)
        return fresh_payload unless File.exist?(path)

        payload = JSON.parse(File.read(path))
        payload["env"] = Env.snapshot.transform_keys(&:to_s) if stale_env?(payload)
        payload
      end

      def fresh_payload
        { "env" => Env.snapshot.transform_keys(&:to_s), "suites" => {} }
      end

      # True when +payload+'s capture stamp predates the round now running,
      # which is how a re-run at the same +(date, sha)+ would otherwise
      # describe its suites by the earlier round's machine state. The lock
      # bounds a round, so the moment it appeared is that round's start; a
      # probe driven directly holds no lock and is a round of its own. The
      # comparison drops to the stamp's own one-second resolution, or a
      # first probe writing within the lock's second would read as stale.
      def stale_env?(payload)
        return true unless File.exist?(@lock)

        captured = payload.dig("env", "captured_at")
        captured.nil? || Time.parse(captured) < File.mtime(@lock).floor
      end
    end
  end
end
