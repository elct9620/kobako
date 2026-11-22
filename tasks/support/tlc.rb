# frozen_string_literal: true

require "etc"
require "fileutils"
require "open3"
require "tmpdir"

# Runs the TLA+ models of a design with TLC, for the tasks that check them.
# Each model's .cfg opens with the outcome it must reach, so a model of a
# broken rule keeps proving that the rule holds the design up. A check
# depends on no other, so they run at once, and none opens a socket. TLC
# needs a JDK, which the gate does not, so those tasks stand apart from the
# default task.
module KobakoTlc
  LAUNCHER = File.expand_path("tlc/LocalTlc.java", __dir__)
  JOBS = 8

  module_function

  # The jar the environment names, or the instruction to name it.
  def jar
    ENV.fetch("TLA2TOOLS_JAR") do
      abort "Set TLA2TOOLS_JAR to the path of tla2tools.jar from the TLA+ tools release."
    end
  end

  # The word +config+ opens with, which must be one of +outcomes+.
  def expectation(config, outcomes)
    expected = File.foreach(config).first[/expect: (\w+)/, 1]
    return expected if outcomes.include?(expected)

    abort "#{config} must open with `\\* expect: ` and one of #{outcomes.join(", ")}."
  end

  # What TLC answers for each config of +specs+, a Hash of config to spec, in
  # its order. +library+ names a directory of modules a spec extends.
  def outcomes(jar, specs, library: nil)
    Dir.mktmpdir do |built|
      classpath = [build_launcher(built), jar].join(File::PATH_SEPARATOR)
      left = Queue.new
      specs.each { |pair| left << pair }
      left.close
      answers = workers(left) { |config, spec| outcome(classpath, config, spec, library) }
      specs.keys.to_h { |config| [config, answers.fetch(config)] }
    end
  end

  # What the block answers for each pair +left+ hands out, by its key.
  def workers(left)
    Array.new([Etc.nprocessors, JOBS].min) do
      Thread.new do
        answered = []
        while (key, value = left.pop)
          answered << [key, yield(key, value)]
        end
        answered
      end
    end.flat_map(&:value).to_h
  end

  def build_launcher(dir)
    output, status = Open3.capture2e("javac", "-d", dir, LAUNCHER)
    abort "The TLC launcher needs a JDK's javac to build: #{output}" unless status.success?

    dir
  end

  def outcome(classpath, config, spec, library)
    Dir.mktmpdir do |dir|
      spec, config = beside(dir, spec, config, library) if library
      # TLC unpacks the standard modules into the temporary directory, so
      # checks that run at once each need their own.
      output, = Open3.capture2e("java", "-Djava.io.tmpdir=#{dir}", "-XX:TieredStopAtLevel=1", "-XX:+UseSerialGC",
                                "-cp", classpath, "LocalTlc", "-workers", "1", "-nowarning",
                                "-metadir", File.join(dir, "states"), "-config", config, spec)
      answer = read(output)
      warn output if answer == "error"
      answer
    end
  end

  # Copies in +dir+ of +spec+, its +config+, and the modules of both its
  # directory and the library, as TLC reads them only from beside the spec.
  def beside(dir, spec, config, library)
    modules = [File.dirname(spec), library].flat_map { |from| Dir.glob(File.join(from, "*.tla")) }
    FileUtils.cp([*modules, config], dir)
    [spec, config].map { |path| File.join(dir, File.basename(path)) }
  end

  # The outcome TLC's output states.
  def read(output)
    return "deadlock" if output.include?("Deadlock reached")
    return "violated" if output.match?(/is violated|properties were violated/)
    return "ok" if output.include?("No error has been found")

    "error"
  end
end
