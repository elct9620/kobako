# frozen_string_literal: true

require "test_helper"

# Regression: a guest-supplied method name must not reach Ruby's ambient
# reflection surface (docs/behavior/security.md B-42).
# Before the guard, method="send" let a guest pivot
# `public_send(:send, :eval, code)` into host RCE; a bound lambda's own
# `Proc#binding` reached `Binding#eval` for the same effect.
class TestDispatchMethodAllowlist < Minitest::Test
  class Service
    def color = "blue"
  end

  # A Service that defines its own public method named +tap+ — a name that is
  # rejected when its owner is +Kernel+. The guard decides on the resolved
  # owner, so this +tap+ (owned by the Service) stays reachable.
  class Tappable
    def tap = "tapped"
  end

  def setup
    @handler = Kobako::Catalog::Handles.new
    @services = Kobako::Catalog::Services.new
    { Theme: Service.new, Fn: ->(x) { x * 2 }, Meth: "abc".method(:upcase), Own: Tappable.new,
      Klass: File, Mod: Kernel }
      .each { |name, service| @services.bind("Cfg::#{name}", service) }
    @services.seal!
    @yield = ->(_bytes) { raise "no block" }
  end

  def dispatch(target, method, args)
    call = DispatcherHelpers.call_for(target, method, args)
    DispatcherHelpers.reify(Kobako::Transport::Dispatcher.dispatch(call, @services, @handler, @yield))
  end

  # @behavior T-116
  def test_meta_methods_are_rejected_not_dispatched
    %w[send __send__ public_send instance_eval instance_exec eval method tap
       instance_variable_get class].each do |meta|
      resp = dispatch("Cfg::Theme", meta, [:eval, "1"])
      assert_equal false, resp.ok?,
                   "method #{meta.inspect} through guest dispatch must be rejected, not invoked on the host"
    end
  end

  # @behavior T-117
  def test_gadget_reflection_methods_are_rejected
    # A Proc / Method bound as a Service exposes reflection on its own type:
    # Proc#binding -> Binding#eval was the reproduced host RCE, and
    # Method#receiver / #unbind hand back the underlying object. None are
    # Service behaviour, so all are rejected.
    { "Cfg::Fn" => %w[binding curry to_proc],
      "Cfg::Meth" => %w[receiver unbind owner to_proc] }.each do |target, methods|
      methods.each do |meth|
        resp = dispatch(target, meth, [])
        assert_equal false, resp.ok?,
                     "#{target}.#{meth} through guest dispatch must be rejected, not invoked on the host"
        assert_equal "undefined", resp.payload.type,
                     "#{target}.#{meth} rejection must surface as the undefined Service-method fault (E-43)"
      end
    end
  end

  # @behavior T-118
  def test_callable_allowlist_still_dispatches
    # A bound lambda / Method stays invocable, and the harmless describers
    # (#arity / #lambda?) remain reachable to aid guest-side debugging.
    [["Cfg::Fn", "call", [21], 42],
     ["Cfg::Fn", "arity", [], 1],
     ["Cfg::Meth", "call", [], "ABC"]].each do |target, meth, args, want|
      resp = dispatch(target, meth, args)
      assert_equal true, resp.ok?,
                   "#{target}.#{meth} (callable allowlist) must stay reachable, not be rejected"
      assert_equal want, resp.payload, "#{target}.#{meth} must return #{want.inspect}"
    end
  end

  # @behavior T-119
  def test_real_service_method_still_dispatches
    resp = dispatch("Cfg::Theme", "color", [])
    assert_equal true, resp.ok?,
                 "a genuine public Service method must remain callable"
    assert_equal "blue", resp.payload
  end

  # @behavior T-120
  def test_rejection_decides_on_owner_not_method_name
    # The guard is owner-based, not a static name list: a Service that defines
    # its own public method named `tap` (owned by the Service, not Kernel) stays
    # reachable, while the same name on a plain Service is rejected as Kernel
    # reflection surface. This pins the B-42 mechanism, not just the denylist.
    own = dispatch("Cfg::Own", "tap", [])
    assert_equal true, own.ok?,
                 "a Service's own `tap` (owner = the Service) must stay reachable, not be rejected by name"
    assert_equal "tapped", own.payload

    inherited = dispatch("Cfg::Theme", "tap", [])
    assert_equal false, inherited.ok?,
                 "`tap` owned by Kernel must be rejected as ambient reflection surface"
    assert_equal "undefined", inherited.payload.type,
                 "the Kernel-owned `tap` rejection must surface as the undefined Service-method fault (E-43)"
  end

  # @behavior T-133
  def test_class_level_methods_on_a_bound_class_are_rejected
    # A Class / Module bound directly as a Service exposes its class-level
    # API, each method owned by the receiver's singleton class — which no
    # fixed core-module list can name. The floor treats a singleton-class
    # owner as ambient surface when the target is itself a Module, so
    # File.popen / File.read / File.new / Kernel.system / Kernel.exec are
    # refused, and a forged Call is bound identically (B-42 is host-side).
    { "Cfg::Klass" => %w[popen read new open],
      "Cfg::Mod" => %w[system exec eval] }.each do |target, methods|
      methods.each do |meth|
        resp = dispatch(target, meth, [])
        assert_equal false, resp.ok?,
                     "#{target}.#{meth} (a class-level method) must be refused, not invoked on the host"
        assert_equal "undefined", resp.payload.type,
                     "#{target}.#{meth} rejection must surface as the undefined Service-method fault (E-43)"
      end
    end
  end

  # @behavior T-121
  def test_absent_method_on_a_valid_target_is_refused_as_undefined
    # A method the Service neither defines nor answers via respond_to? (no
    # method_missing): the floor finds no concrete public method and the
    # respond_to? opt-in is false, so the call is refused. The refusal reuses
    # the opaque type="undefined" — the same the floor gives a reflection
    # method — so a target discloses nothing about which methods it defines
    # (B-42), rather than "argument" or "runtime".
    resp = dispatch("Cfg::Theme", "no_such_method", [])
    assert_equal false, resp.ok?,
                 "a method a plain Service does not define must be refused, not dispatched"
    assert_equal "undefined", resp.payload.type,
                 "an absent method must surface as the opaque undefined fault, not argument or runtime (B-42)"
    assert_match(/no public method/, resp.payload.message,
                 "the refusal must come from the absent-method arm, not the reflection-owner arm")
  end
end
