# frozen_string_literal: true

module Kobako
  # Kobako::SandboxOptions — immutable Value Object holding the four
  # per-Sandbox configuration caps. Built on the +class X <
  # Data.define(...)+ subclass form (the Steep-friendly shape — see
  # +lib/kobako/outcome/panic.rb+).
  #
  # The +initialize+ normalises every cap before delegating to Data's
  # +super+: +timeout+ to Float seconds, +memory_limit+ / +stdout_limit+ /
  # +stderr_limit+ to positive Integer bytes. Each cap is +nil+-disablable
  # (an absent argument takes its DEFAULT; an explicit +nil+ leaves the
  # bound off), so all four behave uniformly. Anything that survives
  # +SandboxOptions.new+ is a wire-ready cap bundle the +Kobako::Runtime+
  # constructor consumes as-is.
  class SandboxOptions < Data.define(:timeout, :memory_limit, :stdout_limit, :stderr_limit)
    # Default wall-clock timeout for a single invocation: 60 seconds.
    DEFAULT_TIMEOUT_SECONDS = 60.0

    # Default cap on the per-invocation guest linear-memory delta:
    # 1 MiB. The mruby image's initial allocation and prior invocations'
    # watermark sit outside this budget.
    DEFAULT_MEMORY_LIMIT = 1 << 20

    # Default per-channel capture ceiling: 1 MiB.
    DEFAULT_OUTPUT_LIMIT = 1 << 20

    def initialize(timeout: DEFAULT_TIMEOUT_SECONDS,
                   memory_limit: DEFAULT_MEMORY_LIMIT,
                   stdout_limit: DEFAULT_OUTPUT_LIMIT,
                   stderr_limit: DEFAULT_OUTPUT_LIMIT)
      timeout = normalize_timeout(timeout)
      memory_limit = normalize_memory_limit(memory_limit)
      stdout_limit = normalize_output_limit(stdout_limit, "stdout_limit")
      stderr_limit = normalize_output_limit(stderr_limit, "stderr_limit")
      super
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

    # Coerce a per-channel output cap (+stdout_limit+ / +stderr_limit+)
    # into the byte cap the ext expects, or +nil+ to leave the channel
    # uncapped. Same shape as +normalize_memory_limit+: a positive Integer
    # when set, Float / zero / negative rejected. +name+ tags the
    # +ArgumentError+ with the offending keyword.
    def normalize_output_limit(limit, name)
      return nil if limit.nil?
      unless limit.is_a?(Integer) && limit.positive?
        raise ArgumentError, "#{name} must be a positive Integer or nil, got #{limit.inspect}"
      end

      limit
    end
  end
end
