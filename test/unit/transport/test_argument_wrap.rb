# frozen_string_literal: true

require "test_helper"

# Coverage for the walk a #run argument takes to the guest: a leaf with no
# wire form becomes a Capability Handle, the rest crosses as it stands.
class TestTransportArgumentWrap < Minitest::Test
  ArgumentWrap = Kobako::Transport::ArgumentWrap

  def setup
    @table = Kobako::Catalog::Handles.new
  end

  # @behavior T-025
  def test_wire_representable_value_passes_through_unchanged
    value = { key: [1, :sym, "x"] }

    wrapped = ArgumentWrap.wrap(value, @table)

    assert_equal value, wrapped
    assert_equal 0, @table.size, "no Handle should be allocated for wire-representable input"
  end

  # @behavior T-026
  def test_non_wire_leaf_is_wrapped_as_a_handle
    body = StringIO.new("hello")

    wrapped = ArgumentWrap.wrap(body, @table)

    assert_kind_of Kobako::Handle, wrapped
    assert_equal 1, @table.size
    assert_same body, @table.fetch(wrapped.id),
                "the allocated entry must point back at the original Ruby object"
  end

  # @behavior T-027
  def test_array_with_mixed_leaves_only_wraps_non_wire_elements
    body = StringIO.new("payload")

    wrapped = ArgumentWrap.wrap([1, body, :sym], @table)

    assert_equal 1, wrapped[0]
    assert_kind_of Kobako::Handle, wrapped[1]
    assert_equal :sym, wrapped[2]
    assert_equal 1, @table.size
  end

  # @behavior T-028
  def test_hash_values_are_walked_keys_pass_through
    env = Object.new

    wrapped = ArgumentWrap.wrap({ env: env, name: "App" }, @table)

    assert_kind_of Kobako::Handle, wrapped[:env]
    assert_equal "App", wrapped[:name]
    assert_equal 1, @table.size
  end

  # @behavior T-029
  def test_non_representable_hash_key_raises_type_error
    err = assert_raises(TypeError) do
      ArgumentWrap.wrap({ StringIO.new("k") => "v" }, @table)
    end

    assert_match(/Hash key/, err.message,
                 "a Hash argument keyed by a value the wire cannot carry must raise TypeError, as " \
                 "Marshal.dump raises for a value with no form, not the codec's own error at encode")
    assert_equal 0, @table.size, "a rejected key must allocate no Handle"
  end

  # @behavior T-030
  def test_existing_handle_is_not_re_wrapped
    original = @table.alloc(:bound)
    pre_size = @table.size

    wrapped = ArgumentWrap.wrap(original, @table)

    assert_same original, wrapped, "an existing Handle is wire-representable and must pass through identity"
    assert_equal pre_size, @table.size, "no extra Handle should be allocated for an existing one"
  end

  # @behavior T-031
  def test_nested_container_is_walked_one_level_at_a_time
    body = StringIO.new("nested")

    wrapped = ArgumentWrap.wrap({ payload: [body, { inner: body }] }, @table)

    inner_array = wrapped[:payload]
    assert_kind_of Kobako::Handle, inner_array[0]
    assert_kind_of Kobako::Handle, inner_array[1][:inner]
    assert_equal 2, @table.size,
                 "each non-wire leaf is wrapped independently; the walk does not de-duplicate object identity"
  end
end
