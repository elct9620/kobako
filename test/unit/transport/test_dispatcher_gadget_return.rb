# frozen_string_literal: true

require "test_helper"

# Regression: a Service returning a reflective gadget must not mint a
# Capability Handle (docs/behavior/security.md B-43).
# Otherwise the guest would receive a callable proxy onto host reflection
# (a returned Binding -> Binding#eval), the second hop of the B-42 escape.
class TestDispatchGadgetReturn < Minitest::Test
  class Service
    def a_method = method(:a_method)
    def a_binding = binding
    def an_unbound = Service.instance_method(:a_method)
    def a_proc = -> { 1 }
  end

  def setup
    @handler = Kobako::Catalog::Handles.new
    @services = Kobako::Catalog::Services.new
    @services.bind("Cfg::S", Service.new)
    @services.seal!
    @yield = ->(_bytes) { raise "no block" }
  end

  def dispatch(method)
    call = DispatcherHelpers.call_for("Cfg::S", method)
    DispatcherHelpers.reify(Kobako::Transport::Dispatcher.dispatch(call, @services, @handler, @yield))
  end

  # @behavior T-122
  def test_reflective_gadget_return_is_refused_not_wrapped
    %w[a_method a_binding an_unbound].each { |meth| assert_gadget_refused(meth) }
  end

  # @behavior T-123
  def test_proc_return_is_still_wrapped_as_handle
    # A Proc stays wrappable (its reflective #binding is blocked by B-42 on
    # the resulting Handle); only Binding / Method / UnboundMethod are refused.
    resp = dispatch("a_proc")
    assert_equal true, resp.ok?,
                 "a returned Proc must still cross as a Capability Handle"
    assert_instance_of Kobako::Handle, resp.payload
  end

  private

  def assert_gadget_refused(meth)
    resp = dispatch(meth)

    assert_equal false, resp.ok?,
                 "a Service returning ##{meth} must not mint a callable Handle onto host reflection"
    assert_equal "runtime", resp.payload.type,
                 "##{meth} gadget return must surface as the runtime fault (E-44)"
    refute_match(/Kobako::/, resp.payload.message,
                 "the refusal of ##{meth} is kobako's own, so it must not wear the " \
                 "<class>: <message> shape a Service exception crosses in")
    assert_equal 0, @handler.size,
                 "##{meth} must allocate no Handle entry"
  end
end
