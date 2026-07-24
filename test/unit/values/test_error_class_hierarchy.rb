# frozen_string_literal: true

require "test_helper"

# Error class hierarchy assertions (SPEC.md F-07). The canonical SPEC
# hierarchy anchors every kobako-raised error under `Kobako::Error`: the
# three invocation-outcome classes plus the construction-layer
# `SetupError` branch.
class TestErrorClassHierarchy < Minitest::Test
  def test_three_top_level_classes_descend_from_kobako_error
    assert Kobako::TrapError < Kobako::Error
    assert Kobako::SandboxError < Kobako::Error
    assert Kobako::ServiceError < Kobako::Error
  end

  # docs/behavior/errors.md E-40 / E-41: SetupError is the construction-layer branch,
  # a sibling of the invocation-outcome classes under Kobako::Error — not a
  # TrapError, because no invocation runs when Sandbox.new fails to build the
  # runtime. ModuleNotBuiltError is its named absent-artifact subclass.
  def test_setup_error_is_a_construction_branch_under_kobako_error
    assert Kobako::SetupError < Kobako::Error
    assert Kobako::ModuleNotBuiltError < Kobako::SetupError
    refute Kobako::SetupError < Kobako::TrapError,
           "construction failures are not invocation traps"
  end

  def test_handler_exhausted_chains_under_sandbox_error
    assert Kobako::HandleExhaustedError < Kobako::SandboxError
  end

  # SPEC E-19 / E-20: TimeoutError and MemoryLimitError are the two named
  # TrapError subclasses for the configured per-run caps from B-01.
  def test_timeout_error_chains_under_trap_error
    assert Kobako::TimeoutError < Kobako::TrapError
    assert Kobako::TimeoutError < Kobako::Error
  end

  def test_memory_limit_error_chains_under_trap_error
    assert Kobako::MemoryLimitError < Kobako::TrapError
    assert Kobako::MemoryLimitError < Kobako::Error
  end
end
