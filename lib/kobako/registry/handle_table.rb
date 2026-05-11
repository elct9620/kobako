# frozen_string_literal: true

module Kobako
  class Registry
    # ===========================================================================
    # Internal class: HandleTable
    #
    # Host-side mapping from opaque integer Handle IDs to Ruby objects
    # (capability proxies). One table is owned per Kobako::Registry instance
    # (and therefore per Kobako::Sandbox instance). See
    # {SPEC.md §HandleTable 實作要點}[link:../../../SPEC.md].
    #
    # Lifecycle invariants ({SPEC.md}[link:../../../SPEC.md]):
    #
    #   - {SPEC.md §B-15}[link:../../../SPEC.md] — Handle IDs are allocated by
    #     a monotonically increasing counter scoped to a single `#run`. The
    #     first ID issued in a run is 1; ID 0 is reserved as the invalid
    #     sentinel and is never returned by #alloc.
    #
    #   - {SPEC.md §B-19}[link:../../../SPEC.md] — When between `#run`
    #     invocations (via `#reset!`), every Handle issued under the old state
    #     becomes invalid.
    #
    #   - {SPEC.md §B-21}[link:../../../SPEC.md] — The cap is `0x7fff_ffff`
    #     (2³¹ − 1). Allocation beyond the cap raises immediately — no silent
    #     truncation, no wrap, no ID reuse.
    # ===========================================================================
    class HandleTable
      # Maximum valid Handle ID. Wire-format invariant:
      # {SPEC.md §B-21}[link:../../../SPEC.md]. 0x7fff_ffff == 2³¹ − 1.
      MAX_ID = 0x7fff_ffff

      # Build a fresh, empty HandleTable.
      #
      # @param next_id [Integer] internal seam: starting value of the
      #   monotonic counter. Defaults to 1 (per B-15). Used by tests to
      #   exercise the cap-exhaustion path without 2³¹ allocations.
      def initialize(next_id: 1)
        @entries = {}
        @next_id = next_id
      end

      # Bind +object+ in the table and return its newly-allocated Handle ID.
      #
      # @param object [Object] host-side Ruby object to bind.
      # @return [Integer] freshly-allocated Handle ID in [1, MAX_ID].
      # @raise [Kobako::HandleTableExhausted] if the cap would be exceeded.
      def alloc(object)
        id = @next_id
        raise HandleTableExhausted, "HandleTable exhausted: id #{id} exceeds MAX_ID #{MAX_ID}" if id > MAX_ID

        @entries[id] = object
        @next_id = id + 1
        id
      end

      # Resolve a Handle ID to its bound object.
      #
      # @param id [Integer] Handle ID previously returned by #alloc.
      # @return [Object] the bound object.
      # @raise [Kobako::HandleTableError] if +id+ is not currently bound.
      def fetch(id)
        return @entries[id] if @entries.key?(id)

        raise HandleTableError, "unknown Handle id: #{id.inspect}"
      end

      # Remove and return the binding for +id+.
      #
      # @param id [Integer] Handle ID to release.
      # @return [Object] the previously-bound object.
      # @raise [Kobako::HandleTableError] if +id+ is not currently bound.
      def release(id)
        raise HandleTableError, "unknown Handle id: #{id.inspect}" unless @entries.key?(id)

        @entries.delete(id)
      end

      # Clear all entries AND reset the counter to 1. Called at the per-run
      # boundary — see {SPEC.md §HandleTable 實作要點, #reset!}[link:../../../SPEC.md].
      #
      # @return [self]
      def reset!
        @entries.clear
        @next_id = 1
        self
      end

      # Mark the entry at +id+ as disconnected (ABA protection).
      #
      # @param id [Integer]
      def mark_disconnected(id)
        @entries[id] = :disconnected if @entries.key?(id)
      end

      # @return [Integer] number of currently-bound entries.
      def size
        @entries.size
      end

      # @param id [Integer]
      # @return [Boolean] whether +id+ is currently bound.
      def include?(id)
        @entries.key?(id)
      end
    end
  end
end
