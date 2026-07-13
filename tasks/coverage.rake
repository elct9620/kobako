# frozen_string_literal: true

# Rake task driving stdlib Coverage measurement against +lib/kobako/+.
# Characterization task, not part of the release gate (+rake default+).
# Run +rake coverage:ruby+ on demand to spot uncovered branches before
# adding new tests or pruning dead code; no thresholds are enforced.
#
# Implementation note: +Coverage.start+ must run BEFORE any +lib/+
# file is required, otherwise lines from that file will have already
# executed and will appear uncovered. The task starts +Coverage+ at
# the top, loads the test suite (which transitively loads +lib/+),
# and emits the report from +Minitest.after_run+ — that hook fires
# after every minitest test completes, so the recorded counts reflect
# the full run.

require_relative "support/coverage"

namespace :coverage do
  desc "Print per-file Ruby line coverage for lib/kobako/ from the full test suite " \
       "(stdlib Coverage; Rust host/guest not measured; not in release gate)."
  task :ruby do
    require "coverage"
    Coverage.start

    $LOAD_PATH.unshift File.expand_path("../test", __dir__)
    require_relative "../test/test_helper"

    Dir.glob(File.expand_path("../test/**/test_*.rb", __dir__)).each { |f| require f }

    Minitest.after_run { puts KobakoCoverage.report_lines(Coverage.result) }
  end
end
