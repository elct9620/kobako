# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — a fillable Service path declared with bind(path) and no
# object, driven through real mruby. The path enters Frame 1 like any bound
# Service (B-08), so it materializes as a guest proxy constant, but it is
# backed by Kobako::Unresolved until the host supplies an object. A guest
# dispatch to an unfilled fillable fails closed as an undefined target,
# surfacing as Kobako::ServiceError when the guest leaves it unrescued (B-62).
class TestE2EFillable < Minitest::Test
  include E2eGuestHelper

  # B-62: reaching the ServiceError proves both halves — the fillable
  # constant exists in the guest (a never-declared constant would raise a
  # guest NameError → SandboxError instead), and the host refused the
  # dispatch to the unfilled sentinel as an unresolved capability.
  def test_dispatch_to_an_unfilled_fillable_raises_service_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Store")

    assert_raises(Kobako::ServiceError,
                  "a guest dispatch to a fillable declared with bind(path) and left unfilled must " \
                  "fail closed as a ServiceError (B-62)") do
      sandbox.eval("Store.get(1)")
    end
  end

  # B-62: bind(path) is sugar for bind(path, Kobako::Unresolved) — the
  # explicit sentinel behaves identically.
  def test_binding_the_unresolved_sentinel_explicitly_matches_the_fillable_sugar
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Store", Kobako::Unresolved)

    assert_raises(Kobako::ServiceError,
                  "bind(path, Kobako::Unresolved) must behave as the fillable default (B-62)") do
      sandbox.eval("Store.get(1)")
    end
  end

  # B-62: an unfilled fillable is observably distinct from a name that was
  # never declared — the fillable's constant exists (dispatch reaches the host
  # → ServiceError), whereas an undeclared name raises a guest NameError that
  # surfaces as SandboxError, never reaching a Service dispatch.
  def test_an_undeclared_name_surfaces_as_sandbox_error_not_service_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Store")

    assert_raises(Kobako::SandboxError,
                  "a guest reference to a never-declared constant must surface as a guest-side " \
                  "SandboxError, distinct from a declared-but-unfilled fillable's ServiceError (B-62)") do
      sandbox.eval("Undeclared.get(1)")
    end
  end

  # B-62: the guest may rescue the capability failure, exactly as it can any
  # Service dispatch fault — leaving it unrescued is what surfaces the host
  # ServiceError, so a rescued call returns normally.
  def test_a_guest_may_rescue_the_unresolved_dispatch_failure
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Store")

    result = sandbox.eval("begin; Store.get(1); :unreached; rescue => e; :rescued; end").value

    assert_equal :rescued, result,
                 "a guest that rescues the fillable dispatch failure runs to completion, so no " \
                 "ServiceError reaches the host (B-62)"
  end
end
