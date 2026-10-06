# frozen_string_literal: true

require "delegate"

require_relative "../handle"
require_relative "../transport/exposure"

module Kobako
  module Catalog
    # One invocation's Handle table, mapping opaque ids to host objects. Each
    # invocation mints its own, so a Handle resolves only where it was issued.
    class Handles # :nodoc:
      # +next_id+ lets tests start near +Kobako::Handle::MAX_ID+ to reach
      # exhaustion without 2³¹ allocations.
      def initialize(next_id: 1)
        @entries = {} # : Hash[Integer, Kobako::Transport::Exposure]
        surfaces = {} # : Hash[Module, Set[Symbol]]
        @surfaces = surfaces.compare_by_identity
        @next_id = next_id
      end

      # The entry records the object's Exposure as it stands at mint, so a
      # call made through this Handle is authorized against the reference
      # the guest was given.
      def alloc(object)
        reject_unwrappable!(object)
        ensure_capacity!
        id = @next_id
        @entries[id] = Kobako::Transport::Exposure.of(object, @surfaces)
        @next_id = id + 1
        Kobako::Handle.restore(id)
      end

      def fetch(id)
        exposure(id).object
      end

      def exposure(id)
        require_bound!(id)
        @entries[id]
      end

      # The table never consults its own size; tests read it to pin how many
      # Handles each path allocates.
      def size
        @entries.size
      end

      private

      # Refuse to mint a Capability Handle for an object whose reachable
      # surface is not Service behaviour: a +Binding+ / +Method+ /
      # +UnboundMethod+ hands the guest a callable proxy onto host
      # reflection (a returned +Binding+ reaches +Binding#eval+); a +Class+
      # or +Module+ hands over its class-level API (+File.popen+ / +read+,
      # +Kernel.system+), which the owner-based dispatch floor cannot see
      # because a singleton-class owner matches no core-module list; a
      # +Delegator+ (+SimpleDelegator+, +DelegateClass+, +WeakRef+,
      # +Tempfile+) is a transparent forwarder whose public +method_missing+
      # binds and calls the private method the guest names (+Kernel#system+),
      # a surface the floor reads as ordinary Service behaviour. Raising here
      # keeps the rule at the single mint point, so it holds on both the
      # Service-return and the +#run+ host→guest auto-wrap paths.
      def reject_unwrappable!(object)
        case object
        when Binding, Method, UnboundMethod, Module, Delegator
          # Delegator < BasicObject exposes no static +#class+, so name the
          # rejected object's class through Object's own.
          kind = Object.instance_method(:class).bind_call(object)
          raise SandboxError, "a #{kind} cannot cross as a Capability Handle"
        end
      end

      def ensure_capacity!
        cap = Kobako::Handle::MAX_ID
        return unless @next_id > cap

        raise HandleExhaustedError,
              "Out of handle allocations: too many host objects were referenced " \
              "in a single invocation (limit #{cap})"
      end

      def require_bound!(id)
        return if @entries.key?(id)

        raise SandboxError, "unknown Handle id: #{id.inspect}"
      end
    end
  end
end
