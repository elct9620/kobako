# frozen_string_literal: true

require_relative "support/tlc"

# The resource models under docs/resources/tla: a module for each tier and
# two for the whole. A .cfg is named for its module, then the rule it takes
# away. It expects ok, violated, or boundary, the last being what is left to
# the host app, which must fail and is counted apart from both.
# The suite has a budget of time, as a tier checked alone adds to it and
# never multiplies it.
namespace :resources do
  budget = 30

  desc "Check the resource models with TLC; TLA2TOOLS_JAR names tla2tools.jar"
  task :check do
    jar = KobakoTlc.jar
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    configs = Dir.glob("docs/resources/tla/*.cfg")
    specs = configs.to_h { |config| [config, "#{config.sub(%r{(-[^/]*)?\.cfg\z}, "")}.tla"] }
    outcomes = KobakoTlc.outcomes(jar, specs, library: "docs/locking/tla")
    met = Hash.new(0)
    failed = configs.reject do |config|
      expected = KobakoTlc.expectation(config, %w[ok violated boundary])
      outcome = outcomes[config]
      puts "#{File.basename(config, ".cfg")}: #{outcome} (expected #{expected})"
      met[expected] += 1 if outcome == (expected == "boundary" ? "violated" : expected)
    end
    took = Process.clock_gettime(Process::CLOCK_MONOTONIC) - started

    puts format("design holds: %<ok>d, a rule taken away fails: %<violated>d, boundary: %<boundary>d, %<took>.1f s",
                ok: met["ok"], violated: met["violated"], boundary: met["boundary"], took: took)
    abort "Resource models off their expected outcome: #{failed.join(", ")}" unless failed.empty?
    abort "Resource models took over #{budget} s, as a design grown too complex does." if took > budget
  end
end
