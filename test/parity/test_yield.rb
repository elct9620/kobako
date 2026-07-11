# frozen_string_literal: true

require "test_helper"

# Differential parity — block yield protocol (SPEC.md B-23..B-30,
# E-21, E-22, E-23): the synchronous yield round-trip, break / next /
# lambda semantics, nested dispatch frames, repeated yields, and the
# escape hatches must observe identically through both frontends.
class TestParityYield < Parity::Case
  YIELD_SERVICE = [
    { name: "MyService::KV",
      methods: { each: { behavior: "yield_each" },
                 answer: { behavior: "value", value: { t: "sym", v: "ok" } } } }
  ].freeze

  # SPEC.md B-23 / B-24 / B-29 / B-30: a guest block reaches the
  # Service, each yield round-trips a value, repeated yields run the
  # block once per iteration, and a Service that never yields discards
  # the block silently.
  def test_yield_round_trip
    assert_parity Parity::Scenario.new(
      name: "yield-round-trip", anchors: %w[B-23 B-24 B-29 B-30],
      services: YIELD_SERVICE,
      invocations: [
        { verb: "eval", source: "MyService::KV.each(1, 2, 3) { |x| x * 10 }" },
        { verb: "eval", source: "MyService::KV.answer { raise 'never runs' }" }
      ]
    )
  end

  # SPEC.md B-25 / B-26 / B-27: `break val` terminates the Service with
  # +val+ as the call's value, `next val` / fallthrough feed the yield
  # site, and a lambda block's `break` behaves as a plain return.
  def test_break_next_semantics
    assert_parity Parity::Scenario.new(
      name: "yield-break-next-lambda", anchors: %w[B-25 B-26 B-27],
      services: YIELD_SERVICE,
      invocations: [
        { verb: "eval", source: "MyService::KV.each(1, 2, 3) { |x| break :stop if x == 2; x * 2 }" },
        { verb: "eval", source: "MyService::KV.each(1, 2) { |x| next x + 5 }" },
        { verb: "eval", source: "MyService::KV.each(7, &->(x) { break x * 3 })" }
      ]
    )
  end

  # SPEC.md B-28: nested dispatch frames each hold their own block; an
  # inner break terminates only the inner Service.
  B28_NESTED_SOURCE = <<~RUBY
    Outer::A.each(1, 2) do |a|
      inner = Inner::B.each(10, 20) { |b| b == 20 ? (break :inner_stop) : b + a }
      [a, inner]
    end
  RUBY

  def test_nested_dispatch
    assert_parity Parity::Scenario.new(
      name: "yield-nested-frames", anchors: %w[B-28],
      services: [
        { name: "Outer::A", methods: { each: { behavior: "yield_each" } } },
        { name: "Inner::B", methods: { each: { behavior: "yield_each" } } }
      ],
      invocations: [{ verb: "eval", source: B28_NESTED_SOURCE }]
    )
  end

  # SPEC.md E-21 / E-22: a block `return` aimed past the yield boundary
  # and an unrepresentable block value both surface at the Service's
  # yield site and, unrescued, attribute to the service origin.
  ESCAPE_INVOCATIONS = [
    { verb: "eval", source: "def leaker; MyService::KV.each(5) { |x| return x }; end; leaker" },
    { verb: "eval",
      source: "def crosser; MyService::KV.each(5) { |x| return x }; rescue => e; e.class.to_s; end; crosser" },
    { verb: "eval", source: "MyService::KV.each(1) { |_x| Object.new }" },
    { verb: "eval",
      source: "begin; MyService::KV.each(1) { |_x| Object.new }; rescue => e; e.class.to_s; end" }
  ].freeze

  def test_yield_escapes
    assert_parity Parity::Scenario.new(
      name: "yield-escapes", anchors: %w[E-21 E-22],
      services: YIELD_SERVICE,
      invocations: ESCAPE_INVOCATIONS
    )
  end

  # SPEC.md E-23: the SDK's +Yielder+ borrows its dispatch frame, so a
  # Service stashing it for a later dispatch is a compile error on the
  # Rust side — no scenario can express the escape there. The Ruby
  # frontend's runtime refusal is pinned by
  # test/e2e/test_yield_unwind.rb.
  def test_escaped_yielder_pending
    skip "E-23 is compile-time-prevented on the SDK Yielder seam; no differential scenario exists"
  end
end
