# frozen_string_literal: true

require "open3"
require "tmpdir"

# The lock models under docs/locking/tla, checked with TLC. Each model's
# .cfg opens with the outcome it must reach, so a model of a broken rule
# keeps proving that the rule holds the order up. TLC needs Java, which the
# gate does not, so this stands apart from the default task.
namespace :locking do
  desc "Check the lock models with TLC; TLA2TOOLS_JAR names tla2tools.jar"
  task :check do
    jar = ENV.fetch("TLA2TOOLS_JAR") do
      abort "Set TLA2TOOLS_JAR to the path of tla2tools.jar from the TLA+ tools release."
    end
    failed = Dir.glob("docs/locking/tla/*.cfg").reject { |config| tlc_meets_expectation?(jar, config) }
    abort "Lock models off their expected outcome: #{failed.join(", ")}" unless failed.empty?
  end

  def tlc_meets_expectation?(jar, config)
    expected = File.foreach(config).first[/expect: (ok|deadlock)/, 1] or
      abort "#{config} must open with `\\* expect: ok` or `\\* expect: deadlock`."
    outcome = Dir.mktmpdir do |meta|
      output, = Open3.capture2e("java", "-cp", jar, "tlc2.TLC", "-workers", "1", "-nowarning",
                                "-metadir", meta, "-config", config, config.sub(/\.cfg\z/, ".tla"))
      tlc_outcome(output)
    end
    puts "#{File.basename(config, ".cfg")}: #{outcome} (expected #{expected})"
    outcome == expected
  end

  def tlc_outcome(output)
    return "deadlock" if output.include?("Deadlock reached")
    return "ok" if output.include?("No error has been found")

    "error"
  end
end
