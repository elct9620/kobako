# frozen_string_literal: true

# E2E + integration test for the pure-Ruby host Catalog::Handles.
#
# Intentionally does NOT require "test_helper" — Catalog::Handles is pure
# Ruby and must be exercisable without the native extension being compiled.
#
# Cross-references:
#   - SPEC.md B-15 — monotonic counter scoped to a single #run, ID 0 reserved
#   - SPEC.md B-19 — Sandbox discard / cross-run Handle invalidity
#   - SPEC.md B-21 — Catalog::Handles exhaustion at 0x7fff_ffff
#   - SPEC.md "Handle Lifecycle" — no finalizer; lifecycle bound to #run

require "minitest/autorun"

$LOAD_PATH.unshift File.expand_path("../../lib", __dir__)
require "kobako/catalog/handles"

module Kobako
  class CatalogHandlerTest < Minitest::Test
    Table = Kobako::Catalog::Handles

    # ---------- Happy path: monotonic allocation, fetch returns identity ----------

    def test_alloc_returns_monotonic_ids_starting_at_one
      table = Table.new
      a = Object.new
      b = Object.new
      c = Object.new

      assert_equal 1, table.alloc(a).id
      assert_equal 2, table.alloc(b).id
      assert_equal 3, table.alloc(c).id
    end

    def test_fetch_returns_the_same_object_that_was_bound
      table = Table.new
      objects = [Object.new, Object.new, Object.new]
      ids = objects.map { |obj| table.alloc(obj).id }

      ids.zip(objects).each { |id, obj| assert_same obj, table.fetch(id) }
    end

    # ---------- Unknown id: fetch raises ----------

    def test_fetch_unknown_id_raises
      table = Table.new
      table.alloc(Object.new) # populates id 1; the binding itself is irrelevant

      assert_raises(Kobako::SandboxError) { table.fetch(999) }
      assert_raises(Kobako::SandboxError) { table.fetch(0) }
    end

    # ---------- Reset: clears entries AND counter (per-#run boundary) ----------

    def test_reset_clears_entries_and_resets_counter_to_one
      table = Table.new
      ids = 5.times.map { table.alloc(Object.new).id }
      assert_equal [1, 2, 3, 4, 5], ids

      table.reset!

      ids.each do |id|
        assert_raises(Kobako::SandboxError) { table.fetch(id) }
      end
      # First alloc after reset returns id 1 — the counter rolls back to the start.
      assert_equal 1, table.alloc(Object.new).id
    end

    def test_reset_on_empty_table_is_noop
      table = Table.new
      table.reset!
      assert_equal 1, table.alloc(Object.new).id
    end

    # ---------- Cap exhaustion: alloc beyond Kobako::Handle::MAX_ID raises ----------

    def test_alloc_at_max_id_succeeds_then_next_alloc_raises
      # Internal seam: next_id: lets us exercise the cap without 2³¹ allocations.
      # Test-only-visible; documented as internal.
      table = Table.new(next_id: Kobako::Handle::MAX_ID)

      id = table.alloc(Object.new).id
      assert_equal Kobako::Handle::MAX_ID, id
      assert_equal 0x7fff_ffff, id

      # SPEC "Error Classes": cap-exhaustion raises the canonical
      # HandlerExhaustedError < SandboxError chain.
      err = assert_raises(Kobako::HandlerExhaustedError) { table.alloc(Object.new) }
      assert_kind_of Kobako::SandboxError, err
    end

    def test_max_id_constant_is_wire_invariant
      # SPEC B-21 + Wire Contract: Handle ext 0x01 carries a 4-byte signed int;
      # 0x7fff_ffff is the maximum valid Handle ID.
      assert_equal 0x7fff_ffff, Kobako::Handle::MAX_ID
      assert_equal (2**31) - 1, Kobako::Handle::MAX_ID
    end

    # ---------- Cross-run Handle invalidity (SPEC B-19) ----------

    def test_handle_from_prior_run_is_invalid_after_reset
      # SPEC B-19: A Handle issued before a reset (the per-#run boundary) must
      # not resolve to its old object after reset. After reset, even if the
      # same numeric id is re-allocated, fetching it must yield the NEW object,
      # not the original — i.e. the original Handle reference is invalidated.
      table = Table.new
      obj_a = Object.new
      table.alloc(obj_a) # binds obj_a at id 1 — the id is asserted below as a literal
      assert_same obj_a, table.fetch(1)

      table.reset!
      obj_b = Object.new
      id_b = table.alloc(obj_b).id

      assert_equal 1, id_b # counter rolled back to 1 at the run boundary
      refute_same obj_a, table.fetch(id_b)
      assert_same obj_b, table.fetch(id_b)
    end
  end
end
