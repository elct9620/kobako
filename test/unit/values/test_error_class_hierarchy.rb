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

  # SPEC E-27: the named subclass for an unresolved `#run` entrypoint. A
  # Host App that only wants "the guest failed" must still catch it with
  # one `rescue Kobako::SandboxError`.
  def test_undefined_entrypoint_chains_under_sandbox_error
    assert Kobako::UndefinedEntrypointError < Kobako::SandboxError
    assert Kobako::UndefinedEntrypointError < Kobako::Error
  end

  # docs/behavior/errors.md § Dispatch failure attribution: a dispatch
  # failure's category picks one of these, and the whole point of putting
  # them under ServiceError is that gaining the distinction costs a Host
  # App nothing — one rescue still catches every Service failure.
  def test_every_dispatch_failure_class_chains_under_service_error
    [Kobako::NoServiceError, Kobako::ServiceArgumentError].each do |subclass|
      assert_operator subclass, :<, Kobako::ServiceError,
                      "#{subclass} through a single rescue Kobako::ServiceError must still be " \
                      "caught, so narrowing a dispatch failure never widens what a caller writes"
    end
  end

  # BlockError is raised at a Service's yield site, inside a dispatch the
  # host is still answering, so it never reaches the Host App as an
  # invocation outcome and must stay outside the three that do.
  def test_block_error_is_not_an_invocation_outcome
    assert_operator Kobako::BlockError, :<, Kobako::Error

    [Kobako::TrapError, Kobako::SandboxError, Kobako::ServiceError].each do |outcome|
      refute_operator Kobako::BlockError, :<, outcome,
                      "BlockError through a rescue of #{outcome} must not be caught — a guest " \
                      "block failing is not one of the invocation's four outcomes"
    end
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
