# frozen_string_literal: true

require "test_helper"

# Differential parity — value round-trip (SPEC.md B-06, B-13, E-06):
# every wire type must deserialize identically through both frontends,
# whether returned by the guest or by a bound Service.
class TestParityValues < Parity::Case
  # SPEC.md B-06: `#eval` last-expression values of every wire type.
  def test_eval_wire_values_round_trip
    sources = [
      "nil", "true", "false", "42", "-7", "3.5", '"text"', ":sym",
      "[1, [2, :a], nil]", '{ answer: 42, "k" => [true, 1.5] }'
    ]
    assert_parity Parity::Scenario.new(
      name: "eval-wire-values", anchors: %w[B-06],
      invocations: sources.map { |source| { verb: "eval", source: } }
    )
  end

  # One host-side constant per wire type a Service can hand the guest.
  SERVICE_CONSTANTS = [
    { "t" => "nil" }, { "t" => "bool", "v" => true },
    { "t" => "int", "v" => "-99" }, { "t" => "float", "v" => 2.25 },
    { "t" => "str", "v" => "from host" }, { "t" => "sym", "v" => "token" },
    { "t" => "array", "v" => [{ "t" => "int", "v" => "1" }, { "t" => "sym", "v" => "x" }] }
  ].freeze

  # SPEC.md B-13: a Service returning wire-representable values —
  # constants flow host→guest, the guest hands them back as its
  # last expression.
  def test_service_values_round_trip
    methods = SERVICE_CONSTANTS.each_with_index.to_h do |constant, index|
      ["value#{index}", { behavior: "value", value: constant }]
    end
    assert_parity Parity::Scenario.new(
      name: "service-values", anchors: %w[B-13],
      services: [{ namespace: "MyService", member: "KV", methods: }],
      invocations: SERVICE_CONSTANTS.each_index.map { |i| { verb: "eval", source: "MyService::KV.value#{i}" } }
    )
  end

  # SPEC.md B-12 / B-13: guest-built values survive the
  # guest→host→guest echo round-trip.
  def test_echo_round_trip
    assert_parity Parity::Scenario.new(
      name: "echo-round-trip", anchors: %w[B-12 B-13],
      services: [{ namespace: "MyService", member: "KV", methods: { echo: { behavior: "echo" } } }],
      invocations: [
        { verb: "eval", source: "MyService::KV.echo({ nested: [1, :a, { deep: true }] })" }
      ]
    )
  end

  # SPEC.md E-06: a return value with no wire representation is a
  # sandbox-origin fault on both sides.
  def test_unrepresentable_return_value
    assert_parity Parity::Scenario.new(
      name: "unrepresentable-return", anchors: %w[E-06],
      invocations: [
        { verb: "eval", source: "Object.new" },
        { verb: "eval", source: "proc { 1 }" }
      ]
    )
  end
end
