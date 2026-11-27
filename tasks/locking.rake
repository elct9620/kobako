# frozen_string_literal: true

require_relative "support/tlc"

# The lock models under docs/locking/tla, each a module beside the .cfg of
# its name, which expects ok, deadlock, or an invariant violated.
namespace :locking do
  desc "Check the lock models with TLC; TLA2TOOLS_JAR names tla2tools.jar"
  task :check do
    jar = KobakoTlc.jar
    configs = Dir.glob("docs/locking/tla/*.cfg")
    outcomes = KobakoTlc.outcomes(jar, configs.to_h { |config| [config, config.sub(/\.cfg\z/, ".tla")] })
    failed = configs.reject do |config|
      expected = KobakoTlc.expectation(config, %w[ok deadlock violated])
      puts "#{File.basename(config, ".cfg")}: #{outcomes[config]} (expected #{expected})"
      outcomes[config] == expected
    end
    abort "Lock models off their expected outcome: #{failed.join(", ")}" unless failed.empty?
  end
end
