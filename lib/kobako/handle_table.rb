# frozen_string_literal: true

# Kobako::HandleTable — host-side mapping from opaque integer Handle IDs to
# Ruby objects (capability proxies). One table is owned per Kobako::Sandbox
# instance (SPEC.md "Capability Handle" / "ext 0x01 — Capability Handle").
#
# This is a pure-Ruby implementation: HandleTable is a hash-plus-counter and
# is not on a performance hotspot, so it does not require a Rust ext.
#
# Lifecycle invariants (cited from SPEC.md):
#
#   - B-15 — Handle IDs are allocated by a monotonically increasing counter
#     scoped to a single `#run`. The first ID issued in a run is 1; ID 0 is
#     reserved as the invalid sentinel and is never returned by #alloc. Within
#     a run the counter never wraps or reuses an ID.
#
#   - B-19 — When the owning Sandbox is discarded (or, equivalently, between
#     `#run` invocations via `#reset!`), every Handle issued under the old
#     state becomes invalid. Cross-run Handle reuse is not supported.
#
#   - B-21 — The cap is `0x7fff_ffff` (2³¹ − 1). This is a wire-format
#     invariant: the Handle ext 0x01 payload is a 4-byte signed integer
#     (mruby on wasm32 defaults to MRB_INT32; values above this cap would
#     silently wrap to a negative integer on the guest side). Allocation
#     beyond the cap raises immediately — no silent truncation, no wrap,
#     no ID reuse.
#
# No finalizer (SPEC "Implementation Standards" — Documentation invariant):
# the host does NOT register `ObjectSpace.define_finalizer` for HandleTable
# entries. Handle release is not driven by host-side Ruby GC. Lifecycle is
# bound to the `#run` boundary via `#reset!`.
#
# Error class: a placeholder `Kobako::HandleTableError < StandardError` is
# defined here. SPEC item #20 is responsible for wiring the canonical
# `Kobako::SandboxError` / `Kobako::HandleTableExhausted` hierarchy; once
# that lands, this file's raise sites should be rewired.

module Kobako
  # Placeholder error class. SPEC item #20 will replace raise sites with
  # `Kobako::HandleTableExhausted` / `Kobako::SandboxError` as appropriate.
  class HandleTableError < StandardError; end

  class HandleTable
    # Maximum valid Handle ID. Wire-format invariant: SPEC.md B-21 and
    # "Wire Contract → Capability Handle". 0x7fff_ffff == 2³¹ − 1.
    MAX_ID = 0x7fff_ffff

    # Build a fresh, empty HandleTable.
    #
    # @param next_id [Integer] internal seam: starting value of the
    #   monotonic counter. Defaults to 1 (per B-15). Used by tests to
    #   exercise the cap-exhaustion path without 2³¹ allocations. Not part
    #   of the public API.
    def initialize(next_id: 1)
      @entries = {}
      @next_id = next_id
    end

    # Bind +object+ in the table and return its newly-allocated Handle ID.
    #
    # The counter is monotonic within a single run: each call returns
    # a strictly greater ID than the previous one (B-15). Allocation
    # beyond MAX_ID raises HandleTableError without writing the entry
    # or advancing the counter (B-21).
    #
    # @param object [Object] host-side Ruby object to bind.
    # @return [Integer] freshly-allocated Handle ID in [1, MAX_ID].
    # @raise [Kobako::HandleTableError] if the cap would be exceeded.
    def alloc(object)
      id = @next_id
      raise HandleTableError, "HandleTable exhausted: id #{id} exceeds MAX_ID #{MAX_ID}" if id > MAX_ID

      @entries[id] = object
      @next_id = id + 1
      id
    end

    # Resolve a Handle ID to its bound object.
    #
    # @param id [Integer] Handle ID previously returned by #alloc within
    #   this same logical run (i.e. since the last #reset!).
    # @return [Object] the bound object.
    # @raise [Kobako::HandleTableError] if +id+ is not currently bound.
    def fetch(id)
      return @entries[id] if @entries.key?(id)

      raise HandleTableError, "unknown Handle id: #{id.inspect}"
    end

    # Remove and return the binding for +id+.
    #
    # Optional internal API: SPEC does not mandate mid-run release. The
    # counter is NOT rolled back — IDs remain monotonic within a run
    # (B-15). For end-of-run bulk clearing use #reset! instead.
    #
    # @param id [Integer] Handle ID to release.
    # @return [Object] the previously-bound object.
    # @raise [Kobako::HandleTableError] if +id+ is not currently bound.
    def release(id)
      raise HandleTableError, "unknown Handle id: #{id.inspect}" unless @entries.key?(id)

      @entries.delete(id)
    end

    # Clear all entries AND reset the counter to 1. Called by Sandbox#run
    # at the per-run boundary (item #17). After #reset!, all previously-
    # issued Handle IDs are invalid (B-19). Distinct from #release: reset
    # rolls the counter back; release does not.
    #
    # @return [self]
    def reset!
      @entries.clear
      @next_id = 1
      self
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
