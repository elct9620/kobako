# frozen_string_literal: true

require "rbconfig"
require "tempfile"

# A Ruby child process a test kills past a deadline. A deadlock leaves no
# Thread of the process able to run a timeout, so code that may deadlock
# runs in a child instead.
module ChildProcess
  # What +script+ prints, run as a child with +args+ under +env+ and the
  # source tree's +lib+ on its load path, or +:deadlocked+ when it outlives
  # +deadline+ seconds. A child that fails flunks with what it wrote to
  # stderr, since nothing else tells a crash from a wrong answer.
  def run_ruby(script, *, env:, deadline:)
    reader, writer = IO.pipe
    Tempfile.create("kobako-child") do |errors|
      pid = spawn_ruby(script, *, env: env, out: writer, err: errors.path)
      writer.close
      next :deadlocked unless finished_within?(pid, deadline)

      flunk "child failed (#{Process.last_status}):\n#{File.read(errors.path)}" unless Process.last_status.success?
      reader.read
    end
  ensure
    reader&.close
  end

  # +script+ started as a child, its output going where +streams+ say.
  def spawn_ruby(script, *, env:, **streams)
    Process.spawn(env, RbConfig.ruby, "-I", TestPaths.source("lib"), "-e", script, *, **streams)
  end

  # Whether +pid+ exited within +deadline+ seconds; one that did not is
  # killed.
  def finished_within?(pid, deadline)
    ends = Process.clock_gettime(Process::CLOCK_MONOTONIC) + deadline
    until Process.wait(pid, Process::WNOHANG)
      next sleep(0.05) if Process.clock_gettime(Process::CLOCK_MONOTONIC) < ends

      Process.kill(:KILL, pid)
      Process.wait(pid)
      return false
    end
    true
  end
end
