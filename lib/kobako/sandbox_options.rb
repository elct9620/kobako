# frozen_string_literal: true

require_relative "errors"

module Kobako
  # The options a Sandbox was built with, read through Sandbox#options or
  # directly on the Sandbox, as in Sandbox#timeout. A cap left out takes its
  # default, and +nil+ switches it off.
  #
  # [+timeout+]       Float seconds one invocation may run.
  # [+memory_limit+]  Integer bytes one invocation may grow guest memory by.
  # [+stdout_limit+, +stderr_limit+]
  #                   Integer bytes kept from each output channel.
  # [+profile+]       The isolation the runtime must provide, one of PROFILES.
  # [+gvl+]           How an invocation holds Ruby's GVL, one of GVL_MODES.
  #
  # Raises ArgumentError for a value outside these shapes, and SetupError
  # when the runtime provides less isolation than +profile+ asks for.
  class SandboxOptions < Data.define(:timeout, :memory_limit, :stdout_limit, :stderr_limit, :profile, :gvl)
    # The default +timeout+: 60 seconds.
    DEFAULT_TIMEOUT_SECONDS = 60.0

    # The default +memory_limit+: 1 MiB of growth past the guest's starting
    # memory.
    DEFAULT_MEMORY_LIMIT = 1 << 20

    # The default +stdout_limit+ and +stderr_limit+: 1 MiB each.
    DEFAULT_OUTPUT_LIMIT = 1 << 20

    # The isolation profiles, weakest first. +:hermetic+ denies the guest the
    # clock and entropy; +:permissive+ leaves them live.
    PROFILES = %i[permissive hermetic].freeze

    # The default +profile+, the strictest.
    DEFAULT_PROFILE = :hermetic

    # +:hold+ keeps the GVL through the invocation; +:release+ drops it while
    # the guest runs, so Sandboxes on different Threads run in parallel.
    GVL_MODES = %i[hold release].freeze

    # The default +gvl+, +:hold+.
    DEFAULT_GVL = :hold

    def initialize(timeout: DEFAULT_TIMEOUT_SECONDS,
                   memory_limit: DEFAULT_MEMORY_LIMIT,
                   stdout_limit: DEFAULT_OUTPUT_LIMIT,
                   stderr_limit: DEFAULT_OUTPUT_LIMIT,
                   profile: DEFAULT_PROFILE,
                   gvl: DEFAULT_GVL)
      timeout = normalize_timeout(timeout)
      memory_limit = normalize_byte_limit(memory_limit, "memory_limit")
      stdout_limit = normalize_byte_limit(stdout_limit, "stdout_limit")
      stderr_limit = normalize_byte_limit(stderr_limit, "stderr_limit")
      profile = normalize_choice(profile, PROFILES, "profile")
      gvl = normalize_choice(gvl, GVL_MODES, "gvl")
      super
    end

    # Both fallbacks fail closed: an unknown declaration ranks below every
    # request, and an unknown request refuses every declaration.
    def enforce_floor!(declared) # :nodoc:
      return if (PROFILES.index(declared) || -1) >= (PROFILES.index(profile) || PROFILES.size)

      raise Kobako::SetupError, "runtime declares isolation profile #{declared.inspect}, " \
                                "below the requested floor #{profile.inspect}"
    end

    private

    # A zero or negative timeout would either fire instantly or never, both
    # more surprising than an early ArgumentError.
    def normalize_timeout(timeout)
      return nil if timeout.nil?
      raise ArgumentError, "timeout must be Numeric or nil, got #{timeout.class}" unless timeout.is_a?(Numeric)

      seconds = timeout.to_f
      raise ArgumentError, "timeout must be > 0 (got #{timeout})" unless seconds.positive? && seconds.finite?

      seconds
    end

    def normalize_byte_limit(limit, name)
      return nil if limit.nil?
      unless limit.is_a?(Integer) && limit.positive?
        raise ArgumentError, "#{name} must be a positive Integer or nil, got #{limit.inspect}"
      end

      limit
    end

    # Unlike the caps there is no +nil+ form: the weakest posture is
    # requested by name, as in +:permissive+.
    def normalize_choice(value, choices, name)
      return value if choices.include?(value)

      raise ArgumentError, "#{name} must be one of #{choices.map(&:inspect).join(", ")}, got #{value.inspect}"
    end
  end
end
