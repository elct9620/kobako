# frozen_string_literal: true

require_relative "errors"

module Kobako
  # The options a Sandbox was built with, read through Sandbox#options or
  # directly on the Sandbox, as in Sandbox#timeout. A cap left out takes its
  # default, and +nil+ switches it off.
  #
  # [+timeout+]       Float seconds one invocation may run.
  # [+setup_timeout+] Float seconds the Guest Binary may take to be set up
  #                   the first time its path is loaded.
  # [+memory_limit+]  Integer bytes one invocation may grow guest memory by.
  # [+stdout_limit+, +stderr_limit+]
  #                   Integer bytes kept from each output channel.
  # [+open_file_limit+]
  #                   Integer descriptors one invocation may hold open, the
  #                   standard ones and each Mount's included.
  # [+profile+]       The isolation the runtime must provide, one of PROFILES.
  # [+gvl+]           How an invocation holds Ruby's GVL, one of GVL_MODES.
  #
  # Raises ArgumentError for a value outside these shapes, and SetupError
  # when the runtime provides less isolation than +profile+ asks for.
  class SandboxOptions < Data.define(:timeout, :setup_timeout, :memory_limit, :stdout_limit, :stderr_limit,
                                     :open_file_limit, :profile, :gvl)
    # The default +timeout+: 60 seconds.
    DEFAULT_TIMEOUT_SECONDS = 60.0

    # The default +setup_timeout+: 10 seconds.
    DEFAULT_SETUP_TIMEOUT_SECONDS = 10.0

    # The default +memory_limit+: 4 MiB of growth past the guest's starting
    # memory, sized for a guest whose every value takes 16 bytes.
    DEFAULT_MEMORY_LIMIT = 4 << 20

    # The default +stdout_limit+ and +stderr_limit+: 1 MiB each.
    DEFAULT_OUTPUT_LIMIT = 1 << 20

    # The default +open_file_limit+: 1024, the soft limit Linux gives a
    # process.
    DEFAULT_OPEN_FILE_LIMIT = 1024

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
                   setup_timeout: DEFAULT_SETUP_TIMEOUT_SECONDS,
                   memory_limit: DEFAULT_MEMORY_LIMIT,
                   stdout_limit: DEFAULT_OUTPUT_LIMIT,
                   stderr_limit: DEFAULT_OUTPUT_LIMIT,
                   open_file_limit: DEFAULT_OPEN_FILE_LIMIT,
                   profile: DEFAULT_PROFILE,
                   gvl: DEFAULT_GVL)
      timeout = normalize_timeout(timeout, "timeout")
      setup_timeout = normalize_timeout(setup_timeout, "setup_timeout")
      memory_limit = normalize_limit(memory_limit, "memory_limit")
      stdout_limit = normalize_limit(stdout_limit, "stdout_limit")
      stderr_limit = normalize_limit(stderr_limit, "stderr_limit")
      open_file_limit = normalize_limit(open_file_limit, "open_file_limit")
      profile = normalize_choice(profile, PROFILES, "profile")
      gvl = normalize_choice(gvl, GVL_MODES, "gvl")
      super
    end

    # The least guest memory one value occupies, so a guest payload may hold
    # one value for each of these bytes of +memory_limit+
    # (docs/wire/payload-msgpack.md § Value Budget).
    VALUE_BYTES = 16

    def value_budget # :nodoc:
      memory_limit&./(VALUE_BYTES)
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
    def normalize_timeout(timeout, name)
      return nil if timeout.nil?
      raise ArgumentError, "#{name} must be Numeric or nil, got #{timeout.class}" unless timeout.is_a?(Numeric)

      seconds = timeout.to_f
      raise ArgumentError, "#{name} must be > 0 (got #{timeout})" unless seconds.positive? && seconds.finite?

      seconds
    end

    def normalize_limit(limit, name)
      return nil if limit.nil?
      unless limit.is_a?(Integer) && limit.positive?
        raise ArgumentError, "#{name} must be a positive Integer or nil, got #{limit.inspect}"
      end

      limit
    end

    # Unlike the caps there is no +nil+ form: every choice, the weakest
    # included, is requested by name, as in +:permissive+.
    def normalize_choice(value, choices, name)
      return value if choices.include?(value)

      raise ArgumentError, "#{name} must be one of #{choices.map(&:inspect).join(", ")}, got #{value.inspect}"
    end
  end
end
