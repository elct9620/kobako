# frozen_string_literal: true

require "test_helper"

# Regression: a guest-supplied method name must not reach Ruby's ambient
# reflection surface. Before the fix, method="send" let a guest pivot
# `public_send(:send, :eval, code)` into host RCE (see Dispatcher#invoke).
class TestDispatchMethodAllowlist < Minitest::Test
  class Service
    def color = "blue"
  end

  def setup
    @handler    = Kobako::Catalog::Handles.new
    @namespaces = Kobako::Catalog::Namespaces.new(handler: @handler)
    @namespaces.define(:Cfg).bind(:Theme, Service.new)
    @namespaces.seal!
    @yield = ->(_bytes) { raise "no block" }
  end

  def dispatch(method, args)
    req = Kobako::Transport::Request.new(target: "Cfg::Theme", method_name: method, args: args)
    bytes = Kobako::Transport::Dispatcher.dispatch(req.encode, @namespaces, @handler, @yield)
    Kobako::Transport::Response.decode(bytes)
  end

  def test_meta_methods_are_rejected_not_dispatched
    %w[send __send__ public_send instance_eval instance_exec eval method tap
       instance_variable_get class].each do |meta|
      resp = dispatch(meta, [:eval, "1"])
      assert_equal Kobako::Transport::STATUS_ERROR, resp.status,
                   "method #{meta.inspect} through guest dispatch must be rejected, not invoked on the host"
    end
  end

  def test_real_service_method_still_dispatches
    resp = dispatch("color", [])
    assert_equal Kobako::Transport::STATUS_OK, resp.status,
                 "a genuine public Service method must remain callable"
    assert_equal "blue", resp.payload
  end
end
