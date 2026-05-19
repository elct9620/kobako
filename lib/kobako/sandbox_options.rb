# frozen_string_literal: true

module Kobako
  # Kobako::SandboxOptions — pure validators for per-Sandbox configuration
  # caps ({docs/behavior.md B-01, E-20}[link:../../docs/behavior.md]).
  #
  # Lives separately from +Kobako::Sandbox+ so the boot-time argument
  # coercion stays a pure function — no instance state, no wasmtime
  # dependency. Sandbox calls these during +#initialize+; the
  # +ArgumentError+ surface is the public contract for ill-formed caps.
  module SandboxOptions
    module_function

    # Coerce +timeout+ into the Float seconds the ext expects, or +nil+ to
    # mean the cap is disabled. Any finite non-positive value is rejected
    # — a zero or negative timeout would either fire instantly or never,
    # both of which would surprise callers more than an early
    # +ArgumentError+.
    def normalize_timeout(timeout)
      return nil if timeout.nil?
      raise ArgumentError, "timeout must be Numeric or nil, got #{timeout.class}" unless timeout.is_a?(Numeric)

      seconds = timeout.to_f
      raise ArgumentError, "timeout must be > 0 (got #{timeout})" unless seconds.positive? && seconds.finite?

      seconds
    end

    # Coerce +memory_limit+ into the byte cap the ext expects, or +nil+ to
    # mean unbounded. Must be a positive Integer when set; Float or
    # zero/negative values are rejected.
    def normalize_memory_limit(memory_limit)
      return nil if memory_limit.nil?
      unless memory_limit.is_a?(Integer) && memory_limit.positive?
        raise ArgumentError, "memory_limit must be a positive Integer or nil, got #{memory_limit.inspect}"
      end

      memory_limit
    end
  end
end
