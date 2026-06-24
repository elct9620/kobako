# frozen_string_literal: true

require "test_helper"

# Coverage for Codec::HandleWalk.deep_restore — the guest→host Handle
# restoration walk introduced for SPEC B-37. The symmetric inverse of
# deep_wrap (test_handle_walk.rb): a value decoded off a guest→host wire
# (the #eval / #run result or a yield-block result) carries Kobako::Handle
# tokens where the guest returned a Handle it held; the walk resolves each
# back to the host object before the Host App or a Service yield site sees
# it.
class TestCodecDeepRestore < Minitest::Test
  HandleWalk = Kobako::Codec::HandleWalk

  def setup
    @table = Kobako::Catalog::Handles.new
  end

  def test_restore_passes_non_handle_values_through_unchanged
    value = { key: [1, :sym, "x"] }

    restored = HandleWalk.deep_restore(value, @table)

    assert_equal value, restored, "a value carrying no Handle must round-trip identically"
  end

  def test_bare_handle_is_restored_to_its_bound_object
    object = Object.new
    handle = @table.alloc(object)

    restored = HandleWalk.deep_restore(handle, @table)

    assert_same object, restored,
                "a returned Handle must resolve back to the host object the guest referenced"
  end

  def test_array_restores_only_handle_elements
    object = Object.new
    handle = @table.alloc(object)

    restored = HandleWalk.deep_restore([1, handle, :sym], @table)

    assert_equal 1, restored[0]
    assert_same object, restored[1]
    assert_equal :sym, restored[2]
  end

  # deep_wrap walks Hash values only (keys arrive as Symbols on the wire),
  # but a decoded return Hash can carry a Handle in either position, so the
  # restore walk covers keys as well.
  def test_hash_restores_both_keys_and_values
    key_object = Object.new
    val_object = Object.new
    key_handle = @table.alloc(key_object)
    val_handle = @table.alloc(val_object)

    restored = HandleWalk.deep_restore({ key_handle => val_handle, plain: "x" }, @table)

    assert_same val_object, restored[key_object],
                "a Handle in either key or value position must be restored to its host object"
    assert_equal "x", restored[:plain]
  end

  def test_nested_container_is_restored_one_level_at_a_time
    object = Object.new
    handle = @table.alloc(object)

    restored = HandleWalk.deep_restore({ payload: [handle, { inner: handle }] }, @table)

    inner_array = restored[:payload]
    assert_same object, inner_array[0]
    assert_same object, inner_array[1][:inner]
  end

  # A Handle the guest cannot forge (B-20) always resolves while its
  # invocation is live; an id with no live binding is the corrupted-runtime
  # fallback B-37 routes to Kobako::SandboxError. reset! drops the binding
  # the way an invocation boundary would.
  def test_handle_with_no_live_binding_raises_sandbox_error
    handle = @table.alloc(Object.new)
    @table.reset!

    assert_raises(Kobako::SandboxError) { HandleWalk.deep_restore(handle, @table) }
  end
end
