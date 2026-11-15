# frozen_string_literal: true

require "test_helper"

# The structural nesting-depth cap on a #run argument's host→guest wrap
# walk. An argument nesting past the maximum encodable depth — a reference
# cycle necessarily does — is refused host-side as the ArgumentError
# +Marshal.dump+ raises past its depth limit, rather than recursing until
# the host stack overflows. The happy-path wrap surface lives in
# test_argument_wrap.rb.
class TestTransportArgumentWrapNesting < Minitest::Test
  ArgumentWrap = Kobako::Transport::ArgumentWrap
  MAX_DEPTH = Kobako::Codec::MAX_NESTING_DEPTH

  def setup
    @table = Kobako::Catalog::Handles.new
  end

  # Wrap +leaf+ in +depth+ nested single-element Arrays.
  def nest(depth, leaf = :leaf)
    depth.times.reduce(leaf) { |acc, _| [acc] }
  end

  # @behavior CD-011
  def test_argument_at_max_nesting_depth_crosses_unchanged
    value = nest(MAX_DEPTH)

    assert_equal value, ArgumentWrap.wrap(value, @table),
                 "a #run argument nested at the maximum encodable depth must cross the boundary unchanged"
  end

  # @behavior CD-012
  # The walk's cap and the guest wire cap name the same bound, so an
  # argument one level past it has no representation to cross in.
  def test_argument_past_max_nesting_depth_raises_argument_error
    err = assert_raises(ArgumentError) do
      ArgumentWrap.wrap(nest(MAX_DEPTH + 1), @table)
    end

    assert_match(/nests deeper than #{MAX_DEPTH} levels/, err.message,
                 "a #run argument nested past the maximum encodable depth must raise ArgumentError " \
                 "naming the depth bound")
  end

  # @behavior CD-013
  # Recursing instead would raise a host SystemStackError, which is not a
  # StandardError, so a bare rescue around #run would not catch it.
  def test_cyclic_argument_raises_argument_error_without_stack_overflow
    cyclic = []
    cyclic << cyclic

    assert_raises(ArgumentError, "a #run argument holding itself must raise ArgumentError, not overflow the stack") do
      ArgumentWrap.wrap(cyclic, @table)
    end
  end

  # @behavior CD-014
  # The key-representability walk is bounded separately from the value
  # one, so a key that refers to itself needs its own witness.
  def test_cyclic_hash_key_raises_type_error_without_stack_overflow
    cyclic = []
    cyclic << cyclic

    assert_raises(TypeError, "a Hash keyed by a value holding itself must raise TypeError, not overflow the stack") do
      ArgumentWrap.wrap({ cyclic => "v" }, @table)
    end
  end
end
