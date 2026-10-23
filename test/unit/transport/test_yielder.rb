# frozen_string_literal: true

require "test_helper"

# Coverage for the host-side Yielder's Handle handling on the ok arm, the
# one Yield Reply arm that carries a value to host code. It is pinned here
# at the seam between the reply the native side split and what the
# Service yield site receives: a Handle is restored to its host object,
# and a payload whose decode carried no Handle reaches the yield site
# unchanged.
class TestYielder < Minitest::Test
  Yielder = Kobako::Transport::Yielder
  BREAK_TAG = :__test_break__

  def setup
    @table = Kobako::Catalog::Handles.new
    @object = Object.new
    @handle = @table.alloc(@object)
  end

  # Build a Yielder whose guest re-entry always answers on +arm+ carrying
  # +value+ (the bound Handle unless overridden), in the +[arm, body,
  # class]+ shape the ext's GuestYielder hands back.
  def yielder_answering(arm, value: @handle)
    reply = [arm, Kobako::Codec::Encoder.encode(value), nil]
    Yielder.new(->(_args) { reply }, BREAK_TAG, @table, Kobako::Transport::BlockFailure.new)
  end

  # @behavior T-100
  def test_ok_value_handle_is_restored_to_its_host_object
    result = yielder_answering(:ok).yield

    assert_same @object, result,
                "a Handle in an ok payload reaches the Service yield site as its host object"
  end

  # @behavior T-101
  def test_handle_free_ok_value_passes_through_unchanged
    result = yielder_answering(:ok, value: ["sum", { "count" => 42 }]).yield

    assert_equal ["sum", { "count" => 42 }], result,
                 "a Handle-free ok payload reaches the Service yield site unchanged"
  end

  # [1, 2, 3] is four values against a budget of three.
  # @behavior T-289
  def test_a_block_answer_past_the_value_budget_raises_at_the_yield
    yielder = yielder_answering(:ok, value: [1, 2, 3])

    error = assert_raises(Kobako::Codec::InvalidTypeError) do
      Kobako::Codec.within_value_budget(3) { yielder.yield }
    end
    assert_equal "payload holds more than 3 values", error.message,
                 "a block answer holding more values than the budget through Yielder#yield must " \
                 "raise at the Service's yield site rather than reach the Service"
  end
end
