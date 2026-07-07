# frozen_string_literal: true

require "test_helper"

# Differential parity — capability Handles (SPEC.md B-14, B-16, B-17,
# B-18, B-20, B-34, B-37, E-13): allocation, nested argument passing,
# chained targets, per-invocation staleness, forge rejection, and
# restore-to-original-object must observe identically through both
# frontends. Opaque host objects carry scenario-declared labels, so
# both executors tag a crossed object by its identity.
class TestParityHandles < Parity::Case
  OPAQUE_SERVICE = [
    { namespace: "Factory", member: "Make",
      methods: { make: { behavior: "opaque", label: "bob" },
                 read_label: { behavior: "read_label" } } }
  ].freeze

  # SPEC.md B-14 / B-16 / B-17 / B-37: Handle allocation, argument
  # passing (bare and Array-nested), chaining a Handle as the next
  # dispatch target, and restoration of the invocation result to the
  # host object's identity.
  LIFECYCLE_INVOCATIONS = [
    { verb: "eval", source: "h = Factory::Make.make; h.label" },
    { verb: "eval", source: "h = Factory::Make.make; Factory::Make.read_label(h)" },
    { verb: "eval", source: "h = Factory::Make.make; Factory::Make.read_label([h])" },
    { verb: "eval", source: "Factory::Make.make" },
    { verb: "eval", source: "h = Factory::Make.make; { list: [h] }" }
  ].freeze

  def test_handle_lifecycle
    assert_parity Parity::Scenario.new(
      name: "handle-lifecycle", anchors: %w[B-14 B-16 B-17 B-37],
      services: OPAQUE_SERVICE,
      invocations: LIFECYCLE_INVOCATIONS
    )
  end

  # SPEC.md B-18 / E-13: with one fresh guest instance per invocation,
  # no guest state — a Handle proxy included — survives the boundary,
  # so no scenario through the real guest can present a stale Handle.
  # Staleness is pinned per-frontend at unit level instead:
  # test/transport/test_dispatcher_invalidity.rb on the Ruby side, the
  # handles/dispatch unit tests in crates/kobako on the SDK side.
  def test_stale_handle_pending
    skip "B-18 E-13 have no guest-expressible differential scenario; staleness is unit-pinned per frontend"
  end

  # SPEC.md B-20: the guest cannot mint a Handle from a raw integer —
  # both construction entries raise, and the failure attributes as an
  # uncaught guest exception on both frontends.
  FORGE_INVOCATIONS = [
    { verb: "eval", source: "Kobako::Handle.new(1)" },
    { verb: "eval", source: "Kobako::Handle.allocate" },
    { verb: "eval", source: "begin; Kobako::Handle.new(1); rescue => e; e.class.to_s; end" }
  ].freeze

  def test_forged_handle
    assert_parity Parity::Scenario.new(
      name: "handle-forge-rejected", anchors: %w[B-20],
      services: OPAQUE_SERVICE,
      invocations: FORGE_INVOCATIONS
    )
  end

  # SPEC.md B-34: a non-wire `#run` argument auto-wraps into a Handle
  # the entrypoint can call back into.
  def test_run_auto_wrap
    assert_parity Parity::Scenario.new(
      name: "run-auto-wrap", anchors: %w[B-34],
      preloads: [
        { kind: "source", name: "Entry",
          code: "class Entry; def self.call(h); h.label; end; end" }
      ],
      invocations: [
        { verb: "run", target: "Entry", args: [{ t: "opaque", label: "tok" }] }
      ]
    )
  end
end
