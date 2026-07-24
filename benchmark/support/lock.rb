# frozen_string_literal: true

require "fileutils"

require_relative "paths"

module Kobako
  module Bench
    # Marks a benchmark run in progress by writing +Paths::LOCK+ with the
    # running process's pid and start time, so the Stop-hook gate can defer
    # its CPU-heavy work while measurements are live rather than contend for
    # cores and skew the numbers. Recording the start time alongside the pid
    # lets a leaked lock — a bench killed before its +ensure+ ran — be told
    # apart from a live one by pid reuse. Re-entrant: a nested +hold+
    # (bench:all driving bench:full) is a no-op, leaving the outermost frame
    # owning and clearing the lock.
    module Lock
      module_function

      # Hold the lock for the duration of the block. The outermost caller
      # writes the lock and removes it on the way out, even on error; a
      # nested caller that already owns it just yields.
      def hold(path = Paths::LOCK)
        return yield if held_by_us?(path)

        write!(path)
        begin
          yield
        ensure
          File.delete(path) if held_by_us?(path)
        end
      end

      # Write +pid+ and the process start time as two lines — the shape the
      # Stop-hook guard reads back to check liveness.
      def write!(path = Paths::LOCK)
        FileUtils.mkdir_p(File.dirname(path))
        File.write(path, "#{Process.pid}\n#{process_start}\n")
      end

      # True when +path+ exists and its pid line names this process — the
      # check the re-entrancy short-circuit and the cleanup guard both hinge
      # on, so +hold+ only ever removes a lock it wrote itself.
      def held_by_us?(path = Paths::LOCK)
        File.exist?(path) && File.read(path).lines.first&.strip == Process.pid.to_s
      end

      # The process start time exactly as +ps+ reports it, matched verbatim
      # by the guard's own +ps -o lstart=+ so a reused pid fails the check.
      def process_start
        `ps -p #{Process.pid} -o lstart=`.strip
      end
    end
  end
end
