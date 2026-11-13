# frozen_string_literal: true

require_relative "errors"

module Kobako
  # The options a Sandbox was built with, read through Sandbox#options or
  # directly on the Sandbox, as in Sandbox#timeout. An option Sandbox.new
  # leaves out takes the runtime's default, given below, and +nil+ switches a
  # cap off.
  #
  # [+timeout+]       Float seconds one invocation may run; 60.
  # [+setup_timeout+] Float seconds the Guest Binary may take to be set up
  #                   the first time its path is loaded; 10.
  # [+memory_limit+]  Integer bytes one invocation may grow guest memory by;
  #                   4 MiB.
  # [+stdout_limit+, +stderr_limit+]
  #                   Integer bytes kept from each output channel; 1 MiB.
  # [+open_file_limit+]
  #                   Integer descriptors one invocation may hold open, the
  #                   standard ones and each Mount's included; 1024.
  # [+open_connection_limit+]
  #                   Integer connections the Sandbox holds open at once, each
  #                   socket, HTTP request, and name lookup in flight one; 6.
  # [+connection_limit+]
  #                   Integer connections one invocation makes in all, each
  #                   socket and HTTP request one; 50.
  # [+profile+]       The isolation the runtime must provide, one of PROFILES;
  #                   +:hermetic+.
  # [+gvl+]           How an invocation holds Ruby's GVL, one of GVL_MODES;
  #                   +:hold+.
  #
  # Raises ArgumentError for a value outside these shapes, and SetupError
  # when the runtime provides less isolation than +profile+ asks for.
  class SandboxOptions < Data.define(:timeout, :setup_timeout, :memory_limit, :stdout_limit, :stderr_limit,
                                     :open_file_limit, :open_connection_limit, :connection_limit, :profile, :gvl)
    # The isolation profiles, weakest first. +:hermetic+ denies the guest the
    # clock and entropy; +:permissive+ leaves them live.
    PROFILES = %i[permissive hermetic].freeze

    # +:hold+ keeps the GVL through the invocation; +:release+ drops it while
    # the guest runs, so Sandboxes on different Threads run in parallel.
    GVL_MODES = %i[hold release].freeze

    # Every option is required; Sandbox.new fills each one left out from
    # the runtime's defaults.
    def initialize(timeout:, setup_timeout:, memory_limit:, stdout_limit:, stderr_limit:, open_file_limit:,
                   open_connection_limit:, connection_limit:, profile:, gvl:)
      timeout = normalize_timeout(timeout, "timeout")
      setup_timeout = normalize_timeout(setup_timeout, "setup_timeout")
      memory_limit, stdout_limit, stderr_limit, open_file_limit, open_connection_limit, connection_limit =
        { memory_limit:, stdout_limit:, stderr_limit:, open_file_limit:, open_connection_limit:, connection_limit: }
        .map { |name, limit| normalize_limit(limit, name.to_s) }
      profile = normalize_choice(profile, PROFILES, "profile")
      gvl = normalize_choice(gvl, GVL_MODES, "gvl")
      super
    end

    # The least guest memory one value occupies, so a guest payload may hold
    # one value for each of these bytes of +memory_limit+.
    VALUE_BYTES = 16
    private_constant :VALUE_BYTES

    def value_budget # :nodoc:
      memory_limit&./(VALUE_BYTES)
    end

    # Both fallbacks fail closed: an unknown declaration ranks below every
    # request, and an unknown request refuses every declaration.
    def enforce_floor(declared) # :nodoc:
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
