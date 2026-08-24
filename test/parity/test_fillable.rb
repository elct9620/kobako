# frozen_string_literal: true

require "test_helper"

# Differential parity — fillable Service paths (SPEC.md B-62) and the per-eval
# ctx.bind override (B-63). Both frontends declare a fillable, observe it fail
# closed while unfilled, and observe the override fill or shadow a binding for
# a single invocation identically.
class TestParityFillable < Parity::Case
  # @behavior SV-017
  # B-62: a fillable declared with no object materializes the guest constant,
  # but a dispatch to it fails closed as a Service failure on both frontends.
  def test_unfilled_fillable_dispatch_fails_closed
    assert_parity Parity::Scenario.new(
      name: "fillable-unfilled", anchors: %w[B-62],
      services: [{ name: "Store", fillable: true }],
      invocations: [{ verb: "eval", source: "Store.get(1)" }]
    )
  end

  # @behavior SV-024
  # B-63: the per-eval override fills the fillable, so both frontends dispatch
  # to the supplied object and observe the same value.
  def test_ctx_bind_override_fills_a_fillable
    assert_parity Parity::Scenario.new(
      name: "fillable-override", anchors: %w[B-63],
      services: [{ name: "Store", fillable: true }],
      invocations: [override_eval("filled")]
    )
  end

  # @behavior SV-026 SV-027
  # B-63: an override shadows a static binding for its own invocation only;
  # the next unadorned eval sees the base binding again, identically on both
  # frontends.
  def test_ctx_bind_override_shadows_a_static_binding_for_one_invocation
    assert_parity Parity::Scenario.new(
      name: "override-shadows-static", anchors: %w[B-63],
      services: [{ name: "Store", methods: { get: { behavior: "value", value: str("base") } } }],
      invocations: [override_eval("shadow"), { verb: "eval", source: "Store.get(1)" }]
    )
  end

  # @behavior SV-025
  # B-63: the override block works on #run too — a preloaded entrypoint whose
  # guest dispatch reaches the object ctx.bind fills, identically on both
  # frontends.
  def test_ctx_bind_override_fills_a_fillable_on_the_run_path
    assert_parity Parity::Scenario.new(
      name: "fillable-override-run", anchors: %w[B-63],
      services: [{ name: "Store", fillable: true }],
      preloads: [{ kind: "source", name: "Worker", code: "Worker = ->(*_a, **_k) { Store.get(1) }" }],
      invocations: [{
        verb: "run", target: "Worker",
        overrides: [{ path: "Store", methods: { get: { behavior: "value", value: str("filled") } } }]
      }]
    )
  end

  private

  def override_eval(text)
    {
      verb: "eval", source: "Store.get(1)",
      overrides: [{ path: "Store", methods: { get: { behavior: "value", value: str(text) } } }]
    }
  end

  def str(text) = { "t" => "str", "v" => text }
end
