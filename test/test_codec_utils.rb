# frozen_string_literal: true

require "stringio"

require "test_helper"

# Coverage for the Codec::Utils predicate and deep-wrap helpers
# introduced for SPEC B-34 — host→guest auto-wrap. The legacy
# +assert_utf8!+ / +wire_boundary+ helpers are exercised transitively
# by the Codec / Decoder / Factory tests; this file pins the new
# allocator-aware surface.
class TestCodecUtils < Minitest::Test
  Utils = Kobako::Codec::Utils

  def setup
    @table = Kobako::HandleTable.new
  end

  # ---------- wire_representable? — scalar branch ----------

  def test_recognises_scalar_wire_types
    [nil, true, false, 0, 1, -1, 1.5, "x", "x".b, :sym].each do |value|
      assert Utils.wire_representable?(value), "expected #{value.inspect} wire-representable"
    end
  end

  def test_recognises_existing_handle_as_wire_representable
    handle = @table.alloc(:placeholder)

    assert Utils.wire_representable?(handle)
  end

  def test_rejects_out_of_range_integers
    refute Utils.wire_representable?(2**64),
           "u64 overflow must be rejected so the codec's RangeError path stays consistent"
    refute Utils.wire_representable?(-(2**63) - 1),
           "below i64 minimum must be rejected"
  end

  def test_rejects_non_wire_scalars
    refute Utils.wire_representable?(StringIO.new("x"))
    refute Utils.wire_representable?(Object.new)
  end

  # ---------- wire_representable? — container branch ----------

  def test_array_is_representable_iff_all_elements_are
    assert Utils.wire_representable?([1, :sym, [true, "x"]])
    refute Utils.wire_representable?([1, StringIO.new("x")])
  end

  def test_hash_is_representable_iff_keys_and_values_are
    assert Utils.wire_representable?({ key: "value", nested: [1, 2] })
    refute Utils.wire_representable?({ key: StringIO.new("x") })
    refute Utils.wire_representable?({ StringIO.new("k") => 1 })
  end

  # ---------- deep_wrap — single-level walk ----------

  def test_wire_representable_value_passes_through_unchanged
    value = { key: [1, :sym, "x"] }

    wrapped = Utils.deep_wrap(value, @table)

    assert_equal value, wrapped
    assert_equal 0, @table.size, "no Handle should be allocated for wire-representable input"
  end

  def test_non_wire_leaf_is_wrapped_via_handle_table
    body = StringIO.new("hello")

    wrapped = Utils.deep_wrap(body, @table)

    assert_kind_of Kobako::Handle, wrapped
    assert_equal 1, @table.size
    assert_same body, @table.fetch(wrapped.id),
                "the allocated entry must point back at the original Ruby object"
  end

  def test_array_with_mixed_leaves_only_wraps_non_wire_elements
    body = StringIO.new("payload")

    wrapped = Utils.deep_wrap([1, body, :sym], @table)

    assert_equal 1, wrapped[0]
    assert_kind_of Kobako::Handle, wrapped[1]
    assert_equal :sym, wrapped[2]
    assert_equal 1, @table.size
  end

  def test_hash_values_are_walked_keys_pass_through
    env = Object.new

    wrapped = Utils.deep_wrap({ env: env, name: "App" }, @table)

    assert_kind_of Kobako::Handle, wrapped[:env]
    assert_equal "App", wrapped[:name]
    assert_equal 1, @table.size
  end

  def test_existing_handle_is_not_re_wrapped
    original = @table.alloc(:bound)
    pre_size = @table.size

    wrapped = Utils.deep_wrap(original, @table)

    assert_same original, wrapped, "an existing Handle is wire-representable and must pass through identity"
    assert_equal pre_size, @table.size, "no extra Handle should be allocated for an existing one"
  end

  def test_nested_container_is_walked_one_level_at_a_time
    body = StringIO.new("nested")

    wrapped = Utils.deep_wrap({ payload: [body, { inner: body }] }, @table)

    inner_array = wrapped[:payload]
    assert_kind_of Kobako::Handle, inner_array[0]
    assert_kind_of Kobako::Handle, inner_array[1][:inner]
    assert_equal 2, @table.size,
                 "each non-wire leaf is wrapped independently; deep_wrap does not de-duplicate object identity"
  end
end
