# frozen_string_literal: true

require "test_helper"

# Regression: a guest-supplied method name must not reach Ruby's ambient
# reflection surface ({docs/behavior.md B-42}[link:../docs/behavior.md]).
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
    @handler    = Kobako::Catalog::Handles.new
    @namespaces = Kobako::Catalog::Namespaces.new(handler: @handler)
    cfg = @namespaces.define(:Cfg)
    { Theme: Service.new, Fn: ->(x) { x * 2 }, Meth: "abc".method(:upcase), Own: Tappable.new }
      .each { |name, service| cfg.bind(name, service) }
    @namespaces.seal!
    @yield = ->(_bytes) { raise "no block" }
  end

  def dispatch(target, method, args)
    req = Kobako::Transport::Request.new(target: target, method_name: method, args: args)
    bytes = Kobako::Transport::Dispatcher.dispatch(req.encode, @namespaces, @handler, @yield)
    Kobako::Transport::Response.decode(bytes)
  end

  def test_meta_methods_are_rejected_not_dispatched
    %w[send __send__ public_send instance_eval instance_exec eval method tap
       instance_variable_get class].each do |meta|
      resp = dispatch("Cfg::Theme", meta, [:eval, "1"])
      assert_equal Kobako::Transport::STATUS_ERROR, resp.status,
                   "method #{meta.inspect} through guest dispatch must be rejected, not invoked on the host"
    end
  end

  def test_gadget_reflection_methods_are_rejected
    # A Proc / Method bound as a Service exposes reflection on its own type:
    # Proc#binding -> Binding#eval was the reproduced host RCE, and
    # Method#receiver / #unbind hand back the underlying object. None are
    # Service behaviour, so all are rejected.
    { "Cfg::Fn" => %w[binding curry to_proc],
      "Cfg::Meth" => %w[receiver unbind owner to_proc] }.each do |target, methods|
      methods.each do |meth|
        resp = dispatch(target, meth, [])
        assert_equal Kobako::Transport::STATUS_ERROR, resp.status,
                     "#{target}.#{meth} through guest dispatch must be rejected, not invoked on the host"
        assert_equal "undefined", resp.payload.type,
                     "#{target}.#{meth} rejection must surface as the undefined Service-method fault (E-43)"
      end
    end
  end

  def test_callable_allowlist_still_dispatches
    # A bound lambda / Method stays invocable, and the harmless describers
    # (#arity / #lambda?) remain reachable to aid guest-side debugging.
    [["Cfg::Fn", "call", [21], 42],
     ["Cfg::Fn", "arity", [], 1],
     ["Cfg::Meth", "call", [], "ABC"]].each do |target, meth, args, want|
      resp = dispatch(target, meth, args)
      assert_equal Kobako::Transport::STATUS_OK, resp.status,
                   "#{target}.#{meth} (callable allowlist) must stay reachable, not be rejected"
      assert_equal want, resp.payload, "#{target}.#{meth} must return #{want.inspect}"
    end
  end

  def test_real_service_method_still_dispatches
    resp = dispatch("Cfg::Theme", "color", [])
    assert_equal Kobako::Transport::STATUS_OK, resp.status,
                 "a genuine public Service method must remain callable"
    assert_equal "blue", resp.payload
  end

  def test_rejection_decides_on_owner_not_method_name
    # The guard is owner-based, not a static name list: a Service that defines
    # its own public method named `tap` (owned by the Service, not Kernel) stays
    # reachable, while the same name on a plain Service is rejected as Kernel
    # reflection surface. This pins the B-42 mechanism, not just the denylist.
    own = dispatch("Cfg::Own", "tap", [])
    assert_equal Kobako::Transport::STATUS_OK, own.status,
                 "a Service's own `tap` (owner = the Service) must stay reachable, not be rejected by name"
    assert_equal "tapped", own.payload

    inherited = dispatch("Cfg::Theme", "tap", [])
    assert_equal Kobako::Transport::STATUS_ERROR, inherited.status,
                 "`tap` owned by Kernel must be rejected as ambient reflection surface"
    assert_equal "undefined", inherited.payload.type,
                 "the Kernel-owned `tap` rejection must surface as the undefined Service-method fault (E-43)"
  end
end
