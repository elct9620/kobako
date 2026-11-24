# frozen_string_literal: true

require_relative "../handle"
require_relative "../transport/exposure"

module Kobako
  module Catalog
    # One invocation's Handle table, mapping opaque ids to host objects. Each
    # invocation mints its own, so a Handle resolves only where it was issued.
    class Handles # :nodoc:
      # The most Handles one invocation issues, as wasmtime's ResourceTable
      # holds at most 1,000,000 of what a host hands its guest: the table is
      # kobako's own, so it stays bounded whatever a Service returns.
      MAX_HANDLES = 1_000_000

      # +next_id+ lets tests start near MAX_HANDLES to reach exhaustion
      # without a million allocations.
      def initialize(next_id: 1)
        @entries = {} # : Hash[Integer, Kobako::Transport::Exposure]
        surfaces = {} # : Hash[Module, Set[Symbol]]
        @surfaces = surfaces.compare_by_identity
        @next_id = next_id
      end

      # The entry records the object's Exposure as it stands at mint, so a
      # call made through this Handle is authorized against the reference
      # the guest was given. The mint point asks the refusal itself, so it
      # holds on every path a Handle is issued.
      def alloc(object)
        refused = Kobako::Transport::Exposure.refused_class_of(object)
        raise SandboxError, "a #{refused} cannot cross as a Capability Handle" if refused

        ensure_capacity
        id = @next_id
        @entries[id] = Kobako::Transport::Exposure.of(object, @surfaces)
        @next_id = id + 1
        Kobako::Handle.restore(id)
      end

      def fetch(id)
        exposure(id).object
      end

      def exposure(id)
        require_bound(id)
        @entries[id]
      end

      # The table never consults its own size; tests read it to pin how many
      # Handles each path allocates.
      def size
        @entries.size
      end

      private

      def ensure_capacity
        return unless @next_id > MAX_HANDLES

        raise HandleExhaustedError,
              "Out of handle allocations: too many host objects were referenced " \
              "in a single invocation (limit #{MAX_HANDLES})"
      end

      def require_bound(id)
        return if @entries.key?(id)

        raise SandboxError, "unknown Handle id: #{id.inspect}"
      end
    end
  end
end
