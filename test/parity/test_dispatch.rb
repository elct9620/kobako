# frozen_string_literal: true

require "test_helper"

# Differential parity — Service dispatch: a guest call on a bound
# constant must produce the same value or the same fault class on both
# sides.
class TestParityDispatch < Parity::Case
  ECHO_SERVICE = [
    { name: "MyService::KV",
      methods: { echo: { behavior: "echo" }, explode: { behavior: "raise", message: "kaput" } } }
  ].freeze

  # Positional args reach the bound constant and its value returns to the
  # guest expression.
  # @behavior T-075
  def test_dispatch_round_trip
    assert_parity Parity::Scenario.new(
      name: "dispatch-round-trip",
      services: ECHO_SERVICE,
      invocations: [{ verb: "eval", source: "MyService::KV.echo([1, :two]) << :three" }]
    )
  end

  # A bound constant that raises surfaces as a rescuable service-origin
  # exception, never a trap.
  # @behavior T-076
  def test_bound_constant_failure_is_rescuable
    assert_parity Parity::Scenario.new(
      name: "dispatch-bound-constant-raise",
      services: ECHO_SERVICE,
      invocations: [
        { verb: "eval", source: "MyService::KV.explode" },
        { verb: "eval", source: "begin; MyService::KV.explode; rescue => e; [e.class.to_s, e.message]; end" }
      ]
    )
  end

  # A method the bound constant does not expose resolves to the undefined
  # fault on both sides.
  # @behavior T-077
  def test_unknown_method_is_undefined
    assert_parity Parity::Scenario.new(
      name: "dispatch-unknown-method",
      services: ECHO_SERVICE,
      invocations: [{ verb: "eval", source: "MyService::KV.not_a_method" }]
    )
  end

  # Keyword arguments offered to a method whose signature accepts none
  # fail the parameter binding as an +argument+ fault on both sides —
  # derived from the stub's positional-only shape, never declared.
  STRICT_SERVICE = [
    { name: "MyService::KV",
      methods: { strict_echo: { behavior: "echo_positional" } } }
  ].freeze

  STRICT_INVOCATIONS = [
    { verb: "eval", source: "MyService::KV.strict_echo(1, limit: 2)" },
    { verb: "eval",
      source: "begin; MyService::KV.strict_echo(1, limit: 2); rescue => e; e.class.to_s; end" },
    { verb: "eval", source: "MyService::KV.strict_echo(1)" }
  ].freeze

  # @behavior T-078
  def test_argument_fault
    assert_parity Parity::Scenario.new(
      name: "dispatch-kwargs-binding-fault",
      services: STRICT_SERVICE,
      invocations: STRICT_INVOCATIONS
    )
  end

  # A service's +exposed+ list narrows the guest-reachable surface on both
  # frontends — an unexposed method is the undefined fault before it runs,
  # an exposed one is unchanged, and the predicate itself is never
  # guest-dispatchable.
  NARROWED_SERVICE = [
    { name: "MyService::KV",
      methods: { visible: { behavior: "echo" }, hidden: { behavior: "echo" } },
      exposed: ["visible"] }
  ].freeze

  NARROWED_INVOCATIONS = [
    { verb: "eval", source: "MyService::KV.visible(1)" },
    { verb: "eval", source: "MyService::KV.hidden(1)" },
    { verb: "eval", source: "begin; MyService::KV.hidden(1); rescue => e; e.class.to_s; end" },
    { verb: "eval", source: "MyService::KV.respond_to_guest?(:visible)" }
  ].freeze

  # @behavior T-079
  def test_respond_to_guest
    assert_parity Parity::Scenario.new(
      name: "dispatch-guest-surface-narrowing",
      services: NARROWED_SERVICE,
      invocations: NARROWED_INVOCATIONS
    )
  end
end
