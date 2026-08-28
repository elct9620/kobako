# frozen_string_literal: true

require_relative "../handle"

module Kobako
  module Catalog
    # Host-side mapping from opaque integer Handle IDs to Ruby objects.
    # Each invocation's +Kobako::Context+ mints its own table and hands it to
    # the dispatcher, so guest→host dispatch resolves Handle targets and
    # arguments against the same table that host→guest wire encoding allocates
    # into for that run.
    #
    # Lifecycle invariants:
    #
    #   - Handle IDs are allocated by a monotonically increasing counter
    #     scoped to a single invocation. The first ID issued in an
    #     invocation is 1; ID 0 is reserved as the invalid sentinel and is
    #     never returned by +#alloc+.
    #
    #   - A fresh table backs each invocation, so a Handle issued in one
    #     invocation resolves in no other — uniformly regardless of
    #     allocation source (Service return or host-injected argument).
    #
    #   - The cap is +0x7fff_ffff+ (2³¹ − 1). Allocation beyond the cap
    #     raises immediately — no silent truncation, no wrap, no ID reuse.
    class Handles
      # Build a fresh, empty table. +next_id+ is an internal seam that
      # sets the starting value of the monotonic counter (defaults to 1);
      # tests pass a value near +Kobako::Handle::MAX_ID+ to exercise
      # the cap-exhaustion path without 2³¹ allocations.
      def initialize(next_id: 1)
        @entries = {} # : Hash[Integer, untyped]
        @next_id = next_id
      end

      # Bind +object+ in the table and return a +Kobako::Handle+ token
      # for it. +object+ is any host-side Ruby object to bind. Returns a
      # freshly-allocated +Kobako::Handle+ whose +#id+ falls in
      # +[Kobako::Handle::MIN_ID, Kobako::Handle::MAX_ID]+. Raises
      # +Kobako::HandleExhaustedError+ if the next ID would exceed the
      # cap. The cap is anchored on +Kobako::Handle+ — the wire codec
      # and the allocator share the same invariant.
      #
      # Returning a Handle (rather than a bare Integer id) keeps the
      # allocator's output a domain entity. An id is the Handle's only
      # content, so the same internal +Kobako::Handle.restore+ constructor
      # serves both this allocator and the codec's wire-decode path.
      def alloc(object)
        reject_unwrappable!(object)
        ensure_capacity!
        id = @next_id
        @entries[id] = object
        @next_id = id + 1
        Kobako::Handle.restore(id)
      end

      # Resolve a Handle ID to its bound object. +id+ is a Handle ID previously
      # returned by +#alloc+. Returns the bound object. Raises
      # +Kobako::SandboxError+ if +id+ is not currently bound.
      def fetch(id)
        require_bound!(id)
        @entries[id]
      end

      # Number of currently-bound entries. Used by tests of the Dispatcher
      # and Codec::HandleWalk#deep_wrap to observe whether each path allocates
      # exactly the Handle entries it should — the +Handles+ table itself never
      # consults its own size, but the surrounding code's allocation
      # contract is part of the observable boundary.
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
      # because a singleton-class owner matches no core-module list. Raising
      # here keeps the rule at the single mint point, so it holds on both the
      # Service-return and the +#run+ host→guest auto-wrap paths.
      def reject_unwrappable!(object)
        case object
        when Binding, Method, UnboundMethod, Module
          raise SandboxError, "a #{object.class} cannot cross as a Capability Handle"
        end
      end

      # Guard #alloc against issuing an ID past the cap. Returns +nil+
      # on success; raises +Kobako::HandleExhaustedError+ at exhaustion.
      def ensure_capacity!
        cap = Kobako::Handle::MAX_ID
        return unless @next_id > cap

        raise HandleExhaustedError,
              "Out of handle allocations: too many host objects were referenced " \
              "in a single invocation (limit #{cap})"
      end

      # Single source of truth for the "unknown Handle id" raise used by
      # #fetch. Returns +nil+ on success; raises +Kobako::SandboxError+
      # when +id+ is not currently bound.
      def require_bound!(id)
        return if @entries.key?(id)

        raise SandboxError, "unknown Handle id: #{id.inspect}"
      end
    end
  end
end
