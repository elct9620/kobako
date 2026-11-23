# frozen_string_literal: true

# A child process the host has no thread or address space left for, through
# the real Guest Binary.
#
# Exhausting the host would starve the test process, so each case is a child
# that lowers its own soft limits and raises them again. RLIMIT_NPROC counts
# the user's threads and binds no root, so the child gives root up first; a
# soft limit of 0 then refuses every new thread of the child alone. Each case
# is a new child, so no guest thread another case left idle answers for one.
# Only Linux limits a process's own threads this way.
module ExhaustedChild
  include ChildProcess

  DEADLINE = 60

  PRELUDE = <<~RUBY.freeze
    require "kobako"
    WASM = ENV.fetch("KOBAKO_WASM")
    #{BLOCKER}

    # What the block answers while the process can start no thread, beside
    # whether Ruby's own Thread.new was refused meanwhile.
    def without_threads
      Process::UID.change_privilege(65_534) if Process.uid.zero?
      soft, hard = Process.getrlimit(:NPROC)
      Process.setrlimit(:NPROC, 0, hard)
      refused = begin
        Thread.new {}.join
        false
      rescue ThreadError
        true
      end
      [refused, yield]
    ensure
      Process.setrlimit(:NPROC, soft, hard)
    end

    # What the block answers while the process has 512 MiB of address space
    # beyond what it maps now.
    def within_address_space
      mapped = File.read("/proc/self/status")[/VmSize:\\s+(\\d+)/, 1].to_i * 1024
      soft, hard = Process.getrlimit(:AS)
      Process.setrlimit(:AS, mapped + (512 << 20), hard)
      yield
    ensure
      Process.setrlimit(:AS, soft, hard)
    end

    # The value +sandbox+ answers for +script+, or the class of the Kobako
    # error it raises.
    def outcome(sandbox, script)
      sandbox.eval(script).value
    rescue Kobako::Error => e
      e.class
    end
  RUBY

  def setup
    super
    skip "only Linux limits a process's own threads" unless RUBY_PLATFORM.include?("linux")
  end

  # What +script+ prints, run after the prelude in a child of its own.
  def run_exhausted_child(script)
    run_ruby(PRELUDE + script, env: { "KOBAKO_WASM" => E2eGuestHelper::REAL_WASM }, deadline: DEADLINE)
  end
end
