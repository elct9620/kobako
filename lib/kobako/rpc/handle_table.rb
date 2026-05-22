# frozen_string_literal: true

require_relative "../handle"

module Kobako
  module RPC
    # Host-side mapping from opaque integer Handle IDs to Ruby objects
    # (capability proxies). One table is owned per +Kobako::RPC::Server+
    # instance (and therefore per +Kobako::Sandbox+ instance). See
    # {docs/behavior.md B-15}[link:../../../docs/behavior.md].
    #
    # Lifecycle invariants ({docs/behavior.md}[link:../../../docs/behavior.md]):
    #
    #   - {docs/behavior.md B-15}[link:../../../docs/behavior.md] — Handle IDs are allocated by
    #     a monotonically increasing counter scoped to a single `#run`. The
    #     first ID issued in a run is 1; ID 0 is reserved as the invalid
    #     sentinel and is never returned by #alloc.
    #
    #   - {docs/behavior.md B-19}[link:../../../docs/behavior.md] — When between `#run`
    #     invocations (via `#reset!`), every Handle issued under the old state
    #     becomes invalid.
    #
    #   - {docs/behavior.md B-21}[link:../../../docs/behavior.md] — The cap is `0x7fff_ffff`
    #     (2³¹ − 1). Allocation beyond the cap raises immediately — no silent
    #     truncation, no wrap, no ID reuse.
    class HandleTable
      # Build a fresh, empty HandleTable. +next_id+ is an internal seam that
      # sets the starting value of the monotonic counter (defaults to 1 per
      # B-15); tests pass a value near +Kobako::Handle::MAX_ID+ to exercise
      # the cap-exhaustion path without 2³¹ allocations.
      def initialize(next_id: 1)
        @entries = {} # : Hash[Integer, untyped]
        @next_id = next_id
      end

      # Bind +object+ in the table and return its newly-allocated Handle ID.
      # +object+ is any host-side Ruby object to bind. Returns a freshly-
      # allocated Handle ID in +[1, Kobako::Handle::MAX_ID]+. Raises
      # +Kobako::HandleTableExhausted+ if the next ID would exceed the cap.
      # The cap is anchored on +Kobako::Handle+ — the wire codec and the
      # allocator share the same invariant ({docs/behavior.md B-21}[link:../../../docs/behavior.md]).
      def alloc(object)
        id = @next_id
        cap = Kobako::Handle::MAX_ID
        raise HandleTableExhausted, "HandleTable exhausted: id #{id} exceeds MAX_ID #{cap}" if id > cap

        @entries[id] = object
        @next_id = id + 1
        id
      end

      # Resolve a Handle ID to its bound object. +id+ is a Handle ID previously
      # returned by +#alloc+. Returns the bound object. Raises
      # +Kobako::HandleTableError+ if +id+ is not currently bound.
      def fetch(id)
        require_bound!(id)
        @entries[id]
      end

      # Remove and return the binding for +id+. +id+ is the Handle ID to
      # release. Returns the previously-bound object. Raises
      # +Kobako::HandleTableError+ if +id+ is not currently bound.
      def release(id)
        require_bound!(id)
        @entries.delete(id)
      end

      # Clear all entries AND reset the counter to 1. Called at the per-run
      # boundary — see {docs/behavior.md B-19}[link:../../../docs/behavior.md].
      # Returns +self+.
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
      # +Kobako::HandleTableError+ when +id+ is not currently bound.
      def require_bound!(id)
        return if @entries.key?(id)

        raise HandleTableError, "unknown Handle id: #{id.inspect}"
      end
    end
  end
end
