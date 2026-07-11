# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — guest-side reflection mirror through real mruby
# (`data/kobako.wasm`). The guest proxy refuses to forward an ambient
# reflection / eval method name to the host ({docs/behavior/security.md
# B-44}[link:../../docs/behavior/security.md]); the callable allowlist still forwards.
#
# B-44 is non-authoritative opacity — the host's B-42 guard is the real
# boundary and is covered host-side in test/transport/test_dispatcher_allowlist.rb.
# This file pins the guest-observable behaviour end to end.
class TestE2EReflectionBlock < Minitest::Test
  include E2eGuestHelper

  def sandbox_with_fn
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Fn", ->(x) { x * 2 })
    sandbox
  end

  def test_reflection_name_is_refused_by_the_guest_proxy
    # A gadget-invoker name reaches the Member proxy's method_missing (it is
    # not a real method on the proxy) and is refused before any wire Request;
    # the uncaught guest NoMethodError surfaces as SandboxError (E-04).
    %w[to_proc curry].each do |meth|
      script = "KV::Fn.#{meth}"
      err = assert_raises(Kobako::SandboxError, "#{script} must be refused guest-side (B-44)") do
        sandbox_with_fn.eval(script)
      end
      assert_match(/#{meth}/, err.message,
                   "the refusal must name the offending reflection method #{meth.inspect}")
    end
  end

  def test_callable_allowlist_forwards_through_the_guest
    # The denylist excludes the callable allowlist, so a bound lambda stays
    # invocable end to end (B-42 / B-44).
    result = sandbox_with_fn.eval("KV::Fn.call(21)")
    assert_equal 42, result,
                 "a bound lambda must remain invocable via #call through the real guest"
  end
end
