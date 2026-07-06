# frozen_string_literal: true

require "test_helper"

# Differential parity — Service dispatch (SPEC.md B-12, E-11, E-12;
# E-15 / B-50 / E-48 pending): a guest call on a bound Member must
# produce the same value or the same fault class on both sides.
class TestParityDispatch < Parity::Case
  ECHO_SERVICE = [
    { namespace: "MyService", member: "KV",
      methods: { echo: { behavior: "echo" }, explode: { behavior: "raise", message: "kaput" } } }
  ].freeze

  # SPEC.md B-12: positional args reach the Member and its value
  # returns to the guest expression.
  def test_dispatch_round_trip
    assert_parity Parity::Scenario.new(
      name: "dispatch-round-trip", anchors: %w[B-12],
      services: ECHO_SERVICE,
      invocations: [{ verb: "eval", source: "MyService::KV.echo([1, :two]) << :three" }]
    )
  end

  # SPEC.md E-11: a Member that raises surfaces as a rescuable
  # service-origin exception, never a trap.
  def test_member_failure_is_rescuable
    assert_parity Parity::Scenario.new(
      name: "dispatch-member-raise", anchors: %w[E-11],
      services: ECHO_SERVICE,
      invocations: [
        { verb: "eval", source: "MyService::KV.explode" },
        { verb: "eval", source: "begin; MyService::KV.explode; rescue => e; [e.class.to_s, e.message]; end" }
      ]
    )
  end

  # SPEC.md E-12: a method the Member does not expose resolves to the
  # undefined fault on both sides.
  def test_unknown_method_is_undefined
    assert_parity Parity::Scenario.new(
      name: "dispatch-unknown-method", anchors: %w[E-12],
      services: ECHO_SERVICE,
      invocations: [{ verb: "eval", source: "MyService::KV.not_a_method" }]
    )
  end

  # SPEC.md E-15: unknown-kwarg param binding faults need signature
  # awareness the SDK's Member seam does not model yet.
  def test_argument_fault_pending
    skip "pending SDK Member signatures (E-15)"
  end

  # SPEC.md B-50 / E-48: the respond_to_guest? narrowing predicate is
  # not modeled on the SDK Member seam yet.
  def test_respond_to_guest_pending
    skip "pending SDK respond_to_guest? seam (B-50 E-48)"
  end
end
