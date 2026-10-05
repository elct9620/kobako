# frozen_string_literal: true

require "test_helper"

# E2E — guest-side reflection mirror through real mruby
# (`data/kobako.wasm`). The guest proxy refuses to forward an ambient
# reflection / eval method name to the host; the callable allowlist still
# forwards.
#
# The guest refusal is non-authoritative opacity — the host's guard is the
# real boundary and is covered host-side in test/unit/transport/test_dispatcher_allowlist.rb.
# This file pins the guest-observable behaviour end to end.
class TestE2EReflectionBlock < Minitest::Test
  include E2eGuestHelper

  def sandbox_with_fn
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Fn", ->(x) { x * 2 })
    sandbox
  end

  # @behavior T-114 T-196
  def test_reflection_name_is_refused_by_the_guest_proxy
    # A gadget-invoker name reaches the bound-constant proxy's method_missing (it is
    # not a real method on the proxy) and is refused before any wire Call;
    # the uncaught guest NoMethodError surfaces as SandboxError.
    %w[to_proc curry].each do |meth|
      script = "KV::Fn.#{meth}"
      err = assert_raises(Kobako::SandboxError, "#{script} must be refused guest-side") do
        sandbox_with_fn.eval(script)
      end
      assert_match(/#{meth}/, err.message,
                   "the refusal must name the offending reflection method #{meth.inspect}")
    end
  end

  # A reference the guest holds is the same proxy shape as a bound
  # constant, so it refuses the same names, and the refusal is one the
  # guest may rescue like any NoMethodError.
  REFUSED_ON_A_REFERENCE = <<~RUBY
    fn = KV::Make.call
    refusals = [-> { fn.to_proc }, -> { fn.curry }].map do |reach|
      reach.call
      :forwarded
    rescue NoMethodError
      :refused
    end
    refusals << fn.call(21)
  RUBY

  # @behavior T-236 T-237
  def test_a_reference_refuses_a_reflective_name_as_a_rescuable_no_method_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Make", -> { ->(x) { x * 2 } })

    assert_equal [:refused, :refused, 42], sandbox.eval(REFUSED_ON_A_REFERENCE).value,
                 "a reflective name on a capability reference through #eval must be refused by " \
                 "the guest proxy as a NoMethodError the guest may rescue"
  end

  # @behavior T-115
  def test_callable_allowlist_forwards_through_the_guest
    # The denylist excludes the callable allowlist, so a bound lambda stays
    # invocable end to end.
    result = sandbox_with_fn.eval("KV::Fn.call(21)").value
    assert_equal 42, result,
                 "a bound lambda must remain invocable via #call through the real guest"
  end
end
