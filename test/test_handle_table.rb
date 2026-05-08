# frozen_string_literal: true

# E2E + integration test for the pure-Ruby host HandleTable (SPEC item #13).
#
# Intentionally does NOT require "test_helper" — HandleTable is pure Ruby and
# must be exercisable without the native extension being compiled. We require
# lib/kobako/handle_table.rb directly.
#
# Cross-references:
#   - SPEC.md B-15 — monotonic counter scoped to a single #run, ID 0 reserved
#   - SPEC.md B-19 — Sandbox discard / cross-run Handle invalidity
#   - SPEC.md B-21 — HandleTable exhaustion at 0x7fff_ffff
#   - SPEC.md "Handle Lifecycle" — no finalizer; lifecycle bound to #run

require "minitest/autorun"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/handle_table"

module Kobako
  class HandleTableTest < Minitest::Test
    Table = Kobako::HandleTable

    # ---------- Happy path: monotonic allocation, fetch returns identity ----------

    def test_alloc_returns_monotonic_ids_starting_at_one
      table = Table.new
      a = Object.new
      b = Object.new
      c = Object.new

      assert_equal 1, table.alloc(a)
      assert_equal 2, table.alloc(b)
      assert_equal 3, table.alloc(c)
    end

    def test_fetch_returns_the_same_object_that_was_bound
      table = Table.new
      a = Object.new
      b = Object.new
      c = Object.new

      id_a = table.alloc(a)
      id_b = table.alloc(b)
      id_c = table.alloc(c)

      assert_same a, table.fetch(id_a)
      assert_same b, table.fetch(id_b)
      assert_same c, table.fetch(id_c)
    end

    # ---------- Unknown id: fetch raises ----------

    def test_fetch_unknown_id_raises
      table = Table.new
      table.alloc(Object.new) # id 1

      assert_raises(Kobako::HandleTableError) { table.fetch(999) }
      assert_raises(Kobako::HandleTableError) { table.fetch(0) }
    end

    # ---------- Release: removes binding; counter does not roll back ----------

    def test_release_removes_binding_and_does_not_reuse_id
      table = Table.new
      obj = Object.new
      id = table.alloc(obj) # 1

      assert_same obj, table.release(id)
      assert_raises(Kobako::HandleTableError) { table.fetch(id) }

      # SPEC B-15: counter is monotonic within a #run; release does not roll back.
      assert_equal 2, table.alloc(Object.new)
    end

    def test_release_unknown_id_raises
      table = Table.new
      assert_raises(Kobako::HandleTableError) { table.release(42) }
    end

    # ---------- Reset: clears entries AND counter (per-#run boundary) ----------

    def test_reset_clears_entries_and_resets_counter_to_one
      table = Table.new
      ids = 5.times.map { table.alloc(Object.new) }
      assert_equal [1, 2, 3, 4, 5], ids

      table.reset!

      ids.each do |id|
        assert_raises(Kobako::HandleTableError) { table.fetch(id) }
      end
      # First alloc after reset returns id 1 — distinct from #release semantics.
      assert_equal 1, table.alloc(Object.new)
    end

    def test_reset_on_empty_table_is_noop
      table = Table.new
      table.reset!
      assert_equal 1, table.alloc(Object.new)
    end

    # ---------- Cap exhaustion: alloc beyond MAX_ID raises ----------

    def test_alloc_at_max_id_succeeds_then_next_alloc_raises
      # Internal seam: next_id: lets us exercise the cap without 2³¹ allocations.
      # Test-only-visible; documented as internal.
      table = Table.new(next_id: Table::MAX_ID)

      id = table.alloc(Object.new)
      assert_equal Table::MAX_ID, id
      assert_equal 0x7fff_ffff, id

      assert_raises(Kobako::HandleTableError) { table.alloc(Object.new) }
    end

    def test_max_id_constant_is_wire_invariant
      # SPEC B-21 + Wire Contract: Handle ext 0x01 carries a 4-byte signed int;
      # 0x7fff_ffff is the maximum valid Handle ID.
      assert_equal 0x7fff_ffff, Table::MAX_ID
      assert_equal 2**31 - 1, Table::MAX_ID
    end

    # ---------- Cross-run Handle invalidity (SPEC B-19) ----------

    def test_handle_from_prior_run_is_invalid_after_reset
      # SPEC B-19: A Handle issued before a reset (the per-#run boundary) must
      # not resolve to its old object after reset. After reset, even if the
      # same numeric id is re-allocated, fetching it must yield the NEW object,
      # not the original — i.e. the original Handle reference is invalidated.
      table = Table.new

      obj_a = Object.new
      id_a = table.alloc(obj_a)
      assert_equal 1, id_a
      assert_same obj_a, table.fetch(id_a)

      table.reset!

      obj_b = Object.new
      id_b = table.alloc(obj_b)
      assert_equal 1, id_b # counter rolled back to 1 at the run boundary

      # The original "id 1" reference is no longer valid for obj_a:
      refute_same obj_a, table.fetch(id_b)
      assert_same obj_b, table.fetch(id_b)
    end

    # ---------- Utility predicates ----------

    def test_size_and_include_predicate
      table = Table.new
      assert_equal 0, table.size
      refute table.include?(1)

      id1 = table.alloc(Object.new)
      id2 = table.alloc(Object.new)
      assert_equal 2, table.size
      assert table.include?(id1)
      assert table.include?(id2)
      refute table.include?(99)

      table.release(id1)
      assert_equal 1, table.size
      refute table.include?(id1)
      assert table.include?(id2)

      table.reset!
      assert_equal 0, table.size
      refute table.include?(id2)
    end

    # ---------- Error class hierarchy sanity ----------

    def test_handle_table_error_is_standard_error_subclass
      # Placeholder until item #20 wires the error class hierarchy.
      assert Kobako::HandleTableError < StandardError
    end
  end
end
