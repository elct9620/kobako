# frozen_string_literal: true

require "rbconfig"

# A Ruby child process a test kills past a deadline. A deadlock leaves no
# Thread of the process able to run a timeout, so code that may deadlock
# runs in a child instead.
module ChildProcess
  # What +script+ prints, run as a child with +args+ under +env+ and the
  # source tree's +lib+ on its load path, or +:deadlocked+ when it outlives
  # +deadline+ seconds.
  def run_ruby(script, *, env:, deadline:)
    reader, writer = IO.pipe
    pid = Process.spawn(env, RbConfig.ruby, "-I", TestPaths.source("lib"),
                        "-e", script, *, out: writer, err: File::NULL)
    writer.close
    finished_within?(pid, deadline) ? reader.read : :deadlocked
  ensure
    reader&.close
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
