# frozen_string_literal: true

require "test_helper"

# Differential parity — Service dispatch (SPEC.md B-12, E-11, E-12,
# E-15, B-50, E-48): a guest call on a bound Member must produce the
# same value or the same fault class on both sides.
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

  # SPEC.md E-15: keyword arguments offered to a method whose signature
  # accepts none fail the parameter binding as an +argument+ fault on
  # both sides — derived from the stub's positional-only shape, never
  # declared.
  STRICT_SERVICE = [
    { namespace: "MyService", member: "KV",
      methods: { strict_echo: { behavior: "echo_positional" } } }
  ].freeze

  STRICT_INVOCATIONS = [
    { verb: "eval", source: "MyService::KV.strict_echo(1, limit: 2)" },
    { verb: "eval",
      source: "begin; MyService::KV.strict_echo(1, limit: 2); rescue => e; e.class.to_s; end" },
    { verb: "eval", source: "MyService::KV.strict_echo(1)" }
  ].freeze

  def test_argument_fault
    assert_parity Parity::Scenario.new(
      name: "dispatch-kwargs-binding-fault", anchors: %w[E-15],
      services: STRICT_SERVICE,
      invocations: STRICT_INVOCATIONS
    )
  end

  # SPEC.md B-50 / E-48: a service's +exposed+ list narrows the
  # guest-reachable surface on both frontends — an unexposed method is
  # the undefined fault before it runs, an exposed one is unchanged,
  # and the predicate itself is never guest-dispatchable.
  NARROWED_SERVICE = [
    { namespace: "MyService", member: "KV",
      methods: { visible: { behavior: "echo" }, hidden: { behavior: "echo" } },
      exposed: ["visible"] }
  ].freeze

  NARROWED_INVOCATIONS = [
    { verb: "eval", source: "MyService::KV.visible(1)" },
    { verb: "eval", source: "MyService::KV.hidden(1)" },
    { verb: "eval", source: "begin; MyService::KV.hidden(1); rescue => e; e.class.to_s; end" },
    { verb: "eval", source: "MyService::KV.respond_to_guest?(:visible)" }
  ].freeze

  def test_respond_to_guest
    assert_parity Parity::Scenario.new(
      name: "dispatch-guest-surface-narrowing", anchors: %w[B-50 E-48],
      services: NARROWED_SERVICE,
      invocations: NARROWED_INVOCATIONS
    )
  end
end
