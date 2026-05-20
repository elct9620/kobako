# frozen_string_literal: true

module Kobako
  # Kobako::SandboxOptions — immutable Value Object holding the four
  # per-Sandbox configuration caps ({docs/behavior.md B-01,
  # E-20}[link:../../docs/behavior.md]). Built on the +class X <
  # Data.define(...)+ subclass form (the Steep-friendly shape — see
  # +lib/kobako/outcome/panic.rb+).
  #
  # The +initialize+ method does double duty: it applies DEFAULT fallback
  # for absent values and normalises (timeout to Float seconds,
  # memory_limit to positive Integer bytes) before delegating to Data's
  # +super+. Anything that survives +SandboxOptions.new+ is a wire-ready
  # cap bundle the +Kobako::Wasm::Instance+ constructor consumes as-is.
  class SandboxOptions < Data.define(:timeout, :memory_limit, :stdout_limit, :stderr_limit)
    # Default wall-clock timeout for a single invocation: 60 seconds
    # ({docs/behavior.md B-01}[link:../../docs/behavior.md]).
    DEFAULT_TIMEOUT_SECONDS = 60.0

    # Default cap on the per-invocation guest linear-memory delta:
    # 1 MiB ({docs/behavior.md B-01}[link:../../docs/behavior.md]).
    # The mruby image's initial allocation and prior invocations'
    # watermark sit outside this budget — see B-01 Notes.
    DEFAULT_MEMORY_LIMIT = 1 << 20

    # Default per-channel capture ceiling: 1 MiB
    # ({docs/behavior.md B-01}[link:../../docs/behavior.md]).
    DEFAULT_OUTPUT_LIMIT = 1 << 20

    # steep:ignore:start
    def initialize(timeout: DEFAULT_TIMEOUT_SECONDS,
                   memory_limit: DEFAULT_MEMORY_LIMIT,
                   stdout_limit: nil,
                   stderr_limit: nil)
      super(
        timeout: normalize_timeout(timeout),
        memory_limit: normalize_memory_limit(memory_limit),
        stdout_limit: stdout_limit || DEFAULT_OUTPUT_LIMIT,
        stderr_limit: stderr_limit || DEFAULT_OUTPUT_LIMIT
      )
    end

    private

    # Coerce +timeout+ into the Float seconds the ext expects, or +nil+
    # to mean the cap is disabled. Any finite non-positive value is
    # rejected — a zero or negative timeout would either fire instantly
    # or never, both of which would surprise callers more than an early
    # +ArgumentError+.
    def normalize_timeout(timeout)
      return nil if timeout.nil?
      raise ArgumentError, "timeout must be Numeric or nil, got #{timeout.class}" unless timeout.is_a?(Numeric)

      seconds = timeout.to_f
      raise ArgumentError, "timeout must be > 0 (got #{timeout})" unless seconds.positive? && seconds.finite?

      seconds
    end

    # Coerce +memory_limit+ into the byte cap the ext expects, or +nil+
    # to mean unbounded. Must be a positive Integer when set; Float or
    # zero/negative values are rejected.
    def normalize_memory_limit(memory_limit)
      return nil if memory_limit.nil?
      unless memory_limit.is_a?(Integer) && memory_limit.positive?
        raise ArgumentError, "memory_limit must be a positive Integer or nil, got #{memory_limit.inspect}"
      end

      memory_limit
    end
    # steep:ignore:end
  end
end
