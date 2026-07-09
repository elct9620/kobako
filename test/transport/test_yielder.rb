# frozen_string_literal: true

require "test_helper"

# Coverage for the host-side Yielder's Handle handling across the two
# value-bearing YieldResponse tags (B-37). The distinction is invisible
# end-to-end — both paths leave the guest with a usable Handle — so it is
# pinned here at the seam between a decoded YieldResponse and what the
# Service yield site receives:
#
#   * 0x01 ok    — the value is consumed by the host Service method, so a
#                  Handle is restored to its host object.
#   * 0x02 break — the value unwinds past the Service back to the guest
#                  Member call (B-25), so a Handle passes through unchanged;
#                  restoring it would churn a fresh Catalog::Handles id.
#
# The Handle-free ok case pins the complement: a payload whose decode
# carried no Handle reaches the yield site unchanged.
class TestYielder < Minitest::Test
  Yielder = Kobako::Transport::Yielder
  BREAK_TAG = :__test_break__

  def setup
    @table = Kobako::Catalog::Handles.new
    @object = Object.new
    @handle = @table.alloc(@object)
  end

  # Build a Yielder whose guest re-entry always answers with a YieldResponse
  # of +tag+ carrying +value+ (the bound Handle unless overridden).
  def yielder_answering(tag, value: @handle)
    bytes = Kobako::Transport::Yield.new(tag: tag, value: value).encode
    Yielder.new(->(_args) { bytes }, BREAK_TAG, @table)
  end

  def test_ok_value_handle_is_restored_to_its_host_object
    result = yielder_answering(Kobako::Transport::TAG_OK).yield

    assert_same @object, result,
                "B-37: a Handle in a 0x01 ok payload reaches the Service yield site as its host object"
  end

  def test_handle_free_ok_value_passes_through_unchanged
    result = yielder_answering(Kobako::Transport::TAG_OK, value: ["sum", { "count" => 42 }]).yield

    assert_equal ["sum", { "count" => 42 }], result,
                 "a Handle-free 0x01 ok payload reaches the Service yield site unchanged"
  end

  def test_break_value_handle_passes_through_without_restoration
    thrown = catch(BREAK_TAG) { yielder_answering(Kobako::Transport::TAG_BREAK).yield }

    assert_kind_of Kobako::Handle, thrown,
                   "B-25/B-37: a break value returns to the guest, so a Handle passes through " \
                   "unrestored rather than resolving to its host object"
    assert_equal @handle.id, thrown.id,
                 "the same Handle id rides back to the guest — no fresh Catalog::Handles entry is churned"
  end
end
