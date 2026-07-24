# frozen_string_literal: true

# E2E + integration test for the pure-Ruby host Catalog::Handles.
#
# Catalog::Handles is pure Ruby and needs no native extension; test_helper's
# no-ext fallback loads the whole pure-Ruby tree (including the
# Kobako::SandboxError / Kobako::HandleExhaustedError this test asserts on),
# so it still runs on a clean checkout.
#
# Cross-references:
#   - SPEC.md B-15 — monotonic counter scoped to a single #run, ID 0 reserved
#   - SPEC.md B-18 — each invocation mints a fresh table; a prior run's id is invalid
#   - SPEC.md B-21 — Catalog::Handles exhaustion at 0x7fff_ffff
#   - SPEC.md "Handle Lifecycle" — no finalizer; lifecycle bound to #run

require "test_helper"

module Kobako
  class CatalogHandlesTest < Minitest::Test
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

    # ---------- Cap exhaustion: alloc beyond Kobako::Handle::MAX_ID raises ----------

    def test_alloc_at_max_id_succeeds_then_next_alloc_raises
      # Internal seam: next_id: lets us exercise the cap without 2³¹ allocations.
      # Test-only-visible; documented as internal.
      table = Table.new(next_id: Kobako::Handle::MAX_ID)

      id = table.alloc(Object.new).id
      assert_equal Kobako::Handle::MAX_ID, id
      assert_equal 0x7fff_ffff, id

      # SPEC "Error Classes": cap-exhaustion raises the canonical
      # HandleExhaustedError < SandboxError chain.
      err = assert_raises(Kobako::HandleExhaustedError) { table.alloc(Object.new) }
      assert_kind_of Kobako::SandboxError, err
    end

    def test_max_id_constant_is_wire_invariant
      # SPEC B-21 + Wire Contract: Handle ext 0x01 carries a 4-byte signed int;
      # 0x7fff_ffff is the maximum valid Handle ID.
      assert_equal 0x7fff_ffff, Kobako::Handle::MAX_ID
      assert_equal (2**31) - 1, Kobako::Handle::MAX_ID
    end

    # ---------- Reflective gadget refusal (SPEC B-43) ----------

    def test_alloc_refuses_reflective_gadgets
      # SPEC B-43: a Binding / Method / UnboundMethod must never be minted as a
      # Capability Handle — wrapping one would hand the guest a callable proxy
      # onto host reflection (a returned Binding reaches Binding#eval). The rule
      # lives here so it holds on both the Service-return and #run auto-wrap paths.
      table = Table.new
      [binding, "abc".method(:upcase), String.instance_method(:upcase)].each do |gadget|
        assert_raises(Kobako::SandboxError) { table.alloc(gadget) }
      end
      assert_equal 0, table.size, "a refused gadget must leave no Handle entry"
    end

    def test_alloc_still_wraps_a_proc
      # A Proc is excluded from the refusal (its reflective #binding is blocked
      # at dispatch, B-42); only Binding / Method / UnboundMethod are unwrappable.
      table = Table.new
      assert_equal 1, table.alloc(-> { 1 }).id
    end

    # ---------- Cross-run Handle invalidity (SPEC B-18) ----------

    def test_a_prior_runs_handle_id_resolves_to_no_object_in_the_next_run
      # SPEC B-18: each invocation mints its own Catalog::Handles, so a Handle
      # issued in one run resolves in no other. The next run's fresh table
      # re-allocates id 1 to its OWN object; the prior binding is unreachable,
      # so the original Handle reference cannot resolve to its old object.
      prior_run = Table.new
      obj_a = Object.new
      prior_run.alloc(obj_a) # binds obj_a at id 1 in the prior run
      assert_same obj_a, prior_run.fetch(1)

      next_run = Table.new
      obj_b = Object.new
      id_b = next_run.alloc(obj_b).id

      assert_equal 1, id_b # the fresh table's counter starts at 1
      assert_same obj_b, next_run.fetch(id_b)
      refute_same obj_a, next_run.fetch(id_b)
    end
  end
end
