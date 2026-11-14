# frozen_string_literal: true

require "test_helper"

# Coverage for the Codec::HandleWalk predicate behind host→guest auto-wrap,
# whose walk is covered in test_argument_wrap.rb. The +deep_restore+ walk is
# covered in test_deep_restore.rb.
class TestCodecHandleWalk < Minitest::Test
  HandleWalk = Kobako::Codec::HandleWalk

  def setup
    @table = Kobako::Catalog::Handles.new
  end

  # ---------- representable? — scalar branch ----------

  # @behavior T-019
  def test_recognises_scalar_wire_types
    [nil, true, false, 0, 1, -1, 1.5, "x", "x".b, :sym].each do |value|
      assert HandleWalk.representable?(value), "expected #{value.inspect} wire-representable"
    end
  end

  # @behavior T-020
  def test_recognises_existing_handle_as_wire_representable
    handle = @table.alloc(:placeholder)

    assert HandleWalk.representable?(handle)
  end

  # @behavior T-021
  def test_rejects_out_of_range_integers
    refute HandleWalk.representable?(2**64),
           "u64 overflow must be rejected so the codec's RangeError path stays consistent"
    refute HandleWalk.representable?(-(2**63) - 1),
           "below i64 minimum must be rejected"
  end

  # @behavior T-022
  def test_rejects_non_wire_scalars
    refute HandleWalk.representable?(StringIO.new("x"))
    refute HandleWalk.representable?(Object.new)
  end

  # ---------- representable? — container branch ----------

  # @behavior T-023
  def test_array_is_representable_iff_all_elements_are
    assert HandleWalk.representable?([1, :sym, [true, "x"]])
    refute HandleWalk.representable?([1, StringIO.new("x")])
  end

  # @behavior T-024
  def test_hash_is_representable_iff_keys_and_values_are
    assert HandleWalk.representable?({ key: "value", nested: [1, 2] })
    refute HandleWalk.representable?({ key: StringIO.new("x") })
    refute HandleWalk.representable?({ StringIO.new("k") => 1 })
  end
end
