# frozen_string_literal: true

require "test_helper"

# Differential parity — capability Handles (SPEC.md B-14, B-16, B-17,
# B-18, B-20, B-34, B-37, E-13): allocation, nested argument passing,
# chained targets, per-invocation staleness, forge rejection, and
# restore-to-original-object must observe identically once the SDK
# grows its Handle table.
class TestParityHandles < Parity::Case
  # SPEC.md B-14 / B-16 / B-17 / B-37: Handle allocation, argument
  # passing, chaining, and restoration.
  def test_handle_lifecycle_pending
    skip "pending SDK Handle table (B-14 B-16 B-17 B-37)"
  end

  # SPEC.md B-18 / E-13: a Handle from invocation N is undefined in
  # invocation N+1.
  def test_stale_handle_pending
    skip "pending SDK Handle table (B-18 E-13)"
  end

  # SPEC.md B-20: neither side forges a Handle from a raw integer.
  def test_forged_handle_pending
    skip "pending SDK Handle table (B-20)"
  end

  # SPEC.md B-34: a non-wire `#run` argument auto-wraps into a Handle.
  def test_run_auto_wrap_pending
    skip "pending SDK run + Handle table (B-34)"
  end
end
