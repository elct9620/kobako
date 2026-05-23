# frozen_string_literal: true

require_relative "../handle"

module Kobako
  module Catalog
    # Host-side mapping from opaque integer Handle IDs to Ruby objects.
    # The table is owned by +Kobako::Sandbox+
    # ({docs/behavior.md B-19}[link:../../../docs/behavior.md]) and injected
    # into the per-Sandbox +Kobako::Catalog::Binding+ so guest→host dispatch
    # resolves Handle targets and arguments against the same table that
    # host→guest wire encoding allocates into
    # ({docs/behavior.md B-14, B-34}[link:../../../docs/behavior.md]).
    #
    # Lifecycle invariants ({docs/behavior.md}[link:../../../docs/behavior.md]):
    #
    #   - {docs/behavior.md B-15}[link:../../../docs/behavior.md] — Handle IDs
    #     are allocated by a monotonically increasing counter scoped to a
    #     single invocation. The first ID issued in an invocation is 1; ID 0
    #     is reserved as the invalid sentinel and is never returned by
    #     +#alloc+.
    #
    #   - {docs/behavior.md B-19}[link:../../../docs/behavior.md] — At every
    #     invocation boundary (via +#reset!+), every Handle issued under the
    #     old state becomes invalid. Reset applies uniformly regardless of
    #     allocation source (B-14 Service return or B-34 host-injected
    #     argument).
    #
    #   - {docs/behavior.md B-21}[link:../../../docs/behavior.md] — The cap is
    #     +0x7fff_ffff+ (2³¹ − 1). Allocation beyond the cap raises
    #     immediately — no silent truncation, no wrap, no ID reuse.
    class Handler
      # Build a fresh, empty Handler. +next_id+ is an internal seam that
      # sets the starting value of the monotonic counter (defaults to 1 per
      # B-15); tests pass a value near +Kobako::Handle::MAX_ID+ to exercise
      # the cap-exhaustion path without 2³¹ allocations.
      def initialize(next_id: 1)
        @entries = {} # : Hash[Integer, untyped]
        @next_id = next_id
      end

      # Bind +object+ in the table and return a +Kobako::Handle+ token
      # for it. +object+ is any host-side Ruby object to bind. Returns a
      # freshly-allocated +Kobako::Handle+ whose +#id+ falls in
      # +[Kobako::Handle::MIN_ID, Kobako::Handle::MAX_ID]+. Raises
      # +Kobako::HandlerExhaustedError+ if the next ID would exceed the
      # cap. The cap is anchored on +Kobako::Handle+ — the wire codec
      # and the allocator share the same invariant
      # ({docs/behavior.md B-21}[link:../../../docs/behavior.md]).
      #
      # Returning a Handle (rather than a bare Integer id) keeps the
      # allocator's output a domain entity; +Kobako::Handle.from_wire+
      # is reserved for the codec's wire-decode path, where the id is
      # the only thing the bytes carry.
      def alloc(object)
        id = @next_id
        cap = Kobako::Handle::MAX_ID
        if id > cap
          raise HandlerExhaustedError,
                "Handle id space exhausted: allocation would assign id #{id}, exceeding the cap (#{cap})"
        end

        @entries[id] = object
        @next_id = id + 1
        Kobako::Handle.from_wire(id)
      end

      # Resolve a Handle ID to its bound object. +id+ is a Handle ID previously
      # returned by +#alloc+. Returns the bound object. Raises
      # +Kobako::SandboxError+ if +id+ is not currently bound.
      def fetch(id)
        require_bound!(id)
        @entries[id]
      end

      # Remove and return the binding for +id+. +id+ is the Handle ID to
      # release. Returns the previously-bound object. Raises
      # +Kobako::SandboxError+ if +id+ is not currently bound.
      def release(id)
        require_bound!(id)
        @entries.delete(id)
      end

      # Clear all entries AND reset the counter to 1. Called at the per-invocation
      # boundary by +Kobako::Sandbox+ — see
      # {docs/behavior.md B-19}[link:../../../docs/behavior.md]. Returns +self+.
      def reset!
        @entries.clear
        @next_id = 1
        self
      end

      # Mark the entry at +id+ as disconnected (ABA protection). +id+ is the
      # Handle ID to poison; silently ignored if +id+ is not currently bound.
      # Returns +self+ for chainability, matching the convention of +#reset!+.
      def mark_disconnected(id)
        @entries[id] = :disconnected if @entries.key?(id)
        self
      end

      # Returns the number of currently-bound entries.
      def size
        @entries.size
      end

      # Returns +true+ when +id+ is currently bound, +false+ otherwise.
      def include?(id)
        @entries.key?(id)
      end

      private

      # Single source of truth for the "unknown Handle id" raise shared by
      # {#fetch} and {#release}. Returns +nil+ on success; raises
      # +Kobako::SandboxError+ when +id+ is not currently bound.
      def require_bound!(id)
        return if @entries.key?(id)

        raise SandboxError, "unknown Handle id: #{id.inspect}"
      end
    end
  end
end
