# frozen_string_literal: true

require "test_helper"

# Error class hierarchy assertions (SPEC.md F-07). The canonical SPEC
# hierarchy anchors every kobako-raised error under `Kobako::Error`: the
# three invocation-outcome classes plus the construction-layer
# `SetupError` branch.
class TestErrorClassHierarchy < Minitest::Test
  # @behavior OC-016
  def test_three_top_level_classes_descend_from_kobako_error
    assert Kobako::TrapError < Kobako::Error
    assert Kobako::SandboxError < Kobako::Error
    assert Kobako::ServiceError < Kobako::Error
  end

  # @behavior OC-017
  # A construction failure is not a trap, because no invocation ran to be
  # cut short — it is a sibling of the outcome classes rather than one
  # of them.
  def test_setup_error_is_a_construction_branch_under_kobako_error
    assert Kobako::SetupError < Kobako::Error
    assert Kobako::ModuleNotBuiltError < Kobako::SetupError
    refute Kobako::SetupError < Kobako::TrapError,
           "construction failures are not invocation traps"
  end

  # @behavior OC-018
  def test_handler_exhausted_chains_under_sandbox_error
    assert Kobako::HandleExhaustedError < Kobako::SandboxError
  end

  # @behavior OC-019
  # A Host App that only wants "the guest failed" must still catch it
  # with one `rescue Kobako::SandboxError`.
  def test_undefined_entrypoint_chains_under_sandbox_error
    assert Kobako::UndefinedEntrypointError < Kobako::SandboxError
    assert Kobako::UndefinedEntrypointError < Kobako::Error
  end

  # @behavior OC-020
  # A dispatch failure's category picks one of these, and the point of
  # putting them under ServiceError is that gaining the distinction costs
  # a Host App nothing — one rescue still catches every Service failure.
  def test_every_dispatch_failure_class_chains_under_service_error
    [Kobako::NoServiceError, Kobako::ServiceArgumentError].each do |subclass|
      assert_operator subclass, :<, Kobako::ServiceError,
                      "#{subclass} through a single rescue Kobako::ServiceError must still be " \
                      "caught, so narrowing a dispatch failure never widens what a caller writes"
    end
  end

  # Both yield-site classes are raised inside a dispatch the host is still
  # answering, where the Service may rescue and go on, so neither reaches
  # the Host App as an invocation outcome and both stay outside the three
  # that do. BlockError carries what the block sent back, YieldValueError
  # what the Service could not send (E-57).
  YIELD_SITE_CLASSES = [Kobako::BlockError, Kobako::YieldValueError].freeze
  INVOCATION_OUTCOMES = [Kobako::TrapError, Kobako::SandboxError, Kobako::ServiceError].freeze

  # @behavior OC-021
  def test_a_yield_site_failure_is_not_an_invocation_outcome
    YIELD_SITE_CLASSES.product(INVOCATION_OUTCOMES).each do |yield_site, outcome|
      assert_operator yield_site, :<, Kobako::Error

      refute_operator yield_site, :<, outcome,
                      "#{yield_site} through a rescue of #{outcome} must not be caught — a " \
                      "failure the Service can rescue at its yield site is not one of the " \
                      "invocation's outcomes"
    end
  end

  # @behavior OC-022
  # The two configured per-run caps are the named trap subclasses, so a
  # Host App can tell a cap apart from an engine trap without losing the
  # single rescue that covers both.
  def test_timeout_error_chains_under_trap_error
    assert Kobako::TimeoutError < Kobako::TrapError
    assert Kobako::TimeoutError < Kobako::Error
  end

  # @behavior OC-023
  def test_memory_limit_error_chains_under_trap_error
    assert Kobako::MemoryLimitError < Kobako::TrapError
    assert Kobako::MemoryLimitError < Kobako::Error
  end
end
