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
  # [+profile+]       The isolation the runtime must provide at least:
  #                   +:hermetic+, which denies the guest the clock and
  #                   entropy, or the weaker +:permissive+, which leaves them
  #                   live; +:hermetic+.
  # [+gvl+]           How an invocation holds Ruby's GVL: +:hold+ through
  #                   the invocation, or +:release+ while the guest runs, so
  #                   Sandboxes on different Threads run in parallel; +:hold+.
  # [+network+]       What the guest's network dials and looks names up
  #                   through, beneath every rule its entries make: +nil+
  #                   for the host's own, or a Kobako::Network::Mock a test
  #                   writes; +nil+.
  #
  # Raises ArgumentError for a cap outside these shapes. The runtime judges
  # every cap, and +profile+, +gvl+, and +network+ as Sandbox.new builds it.
  class SandboxOptions < Data.define(:timeout, :setup_timeout, :memory_limit, :stdout_limit, :stderr_limit,
                                     :open_file_limit, :open_connection_limit, :connection_limit, :profile, :gvl,
                                     :network)
    # Every option is required; Sandbox.new fills each one left out from
    # the runtime's defaults.
    def initialize(timeout:, setup_timeout:, memory_limit:, stdout_limit:, stderr_limit:, open_file_limit:,
                   open_connection_limit:, connection_limit:, profile:, gvl:, network:)
      caps = Kobako::Runtime.normalize_caps({ timeout:, setup_timeout:, memory_limit:, stdout_limit:, stderr_limit:,
                                              open_file_limit:, open_connection_limit:, connection_limit: })
      super(**caps.merge(profile:, gvl:, network:))
    end

    def value_budget # :nodoc:
      Kobako::Runtime.value_budget_of(memory_limit)
    end
  end
end
