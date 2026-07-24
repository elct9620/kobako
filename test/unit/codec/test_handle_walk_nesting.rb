# frozen_string_literal: true

require "test_helper"

# The structural nesting-depth cap on Codec::HandleWalk's host→guest wrap
# walk (E-54). A #run argument nesting past the maximum encodable depth — a
# reference cycle necessarily does — is refused host-side as a clean
# SandboxError rather than recursing until the host stack overflows. The
# happy-path wrap surface lives in test_handle_walk.rb.
class TestCodecHandleWalkNesting < Minitest::Test
  HandleWalk = Kobako::Codec::HandleWalk
  MAX_DEPTH = Kobako::Codec::MAX_NESTING_DEPTH

  def setup
    @table = Kobako::Catalog::Handles.new
  end

  # Wrap +leaf+ in +depth+ nested single-element Arrays.
  def nest(depth, leaf = :leaf)
    depth.times.reduce(leaf) { |acc, _| [acc] }
  end

  def test_argument_at_max_nesting_depth_crosses_unchanged
    value = nest(MAX_DEPTH)

    assert_equal value, HandleWalk.deep_wrap(value, @table),
                 "a #run argument nested at the maximum encodable depth must cross the boundary unchanged"
  end

  # The depth cap and the guest wire cap name the same bound, so an argument
  # one level past it has no faithful representation and is refused host-side.
  def test_argument_past_max_nesting_depth_is_rejected_as_sandbox_error
    err = assert_raises(Kobako::SandboxError) do
      HandleWalk.deep_wrap(nest(MAX_DEPTH + 1), @table)
    end

    assert_match(/nests deeper than #{MAX_DEPTH} levels/, err.message,
                 "a #run argument nested past the maximum encodable depth must be refused as a clean SandboxError")
  end

  # A reference cycle nests without bound; the wrap walk must refuse it as a
  # bounded SandboxError rather than recurse into a host SystemStackError,
  # which is not a StandardError and would escape Sandbox#invoke! uncaught.
  def test_cyclic_argument_is_rejected_without_stack_overflow
    cyclic = []
    cyclic << cyclic

    assert_raises(Kobako::SandboxError) { HandleWalk.deep_wrap(cyclic, @table) }
  end

  # The key-representability walk is depth-bounded too, so a cyclic Hash key
  # is refused as a SandboxError rather than overflowing the predicate walk.
  def test_cyclic_hash_key_is_rejected_without_stack_overflow
    cyclic = []
    cyclic << cyclic

    assert_raises(Kobako::SandboxError) { HandleWalk.deep_wrap({ cyclic => "v" }, @table) }
  end
end
