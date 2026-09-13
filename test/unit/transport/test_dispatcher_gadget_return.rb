# frozen_string_literal: true

require "test_helper"
require "delegate"

# Regression: a Service returning a reflective gadget must not mint a
# Capability Handle. Otherwise the guest would receive a callable proxy onto
# host reflection (a returned Binding -> Binding#eval), the second hop of the
# reflection escape.
class TestDispatchGadgetReturn < Minitest::Test
  class Service
    def a_method = method(:a_method)
    def a_binding = binding
    def an_unbound = Service.instance_method(:a_method)
    def a_proc = -> { 1 }
    def a_class = File
    def a_module = Kernel
    def a_forwarder = SimpleDelegator.new(Object.new)
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

  # @behavior T-122 T-195 T-201
  def test_reflective_gadget_return_is_refused_not_wrapped
    %w[a_method a_binding an_unbound].each { |meth| assert_gadget_refused(meth) }
  end

  # @behavior T-132 T-195
  def test_class_or_module_return_is_refused_not_wrapped
    # A bare Class / Module used as a type tag must not mint a Handle: its
    # class-level API (File.popen / Kernel.system) is owned by a singleton
    # class the dispatch floor cannot enumerate, so the mint point refuses it.
    %w[a_class a_module].each do |meth|
      resp = dispatch(meth)

      assert_equal false, resp.ok?,
                   "a Service returning ##{meth} (a bare Class/Module) must not mint a Handle onto its class-level API"
      assert_equal "runtime", resp.payload.type,
                   "##{meth} return must surface as the runtime fault"
      assert_equal 0, @handler.size,
                   "##{meth} must allocate no Handle entry"
    end
  end

  # @behavior T-206
  def test_transparent_forwarder_return_is_refused_not_wrapped
    # A transparent forwarder (SimpleDelegator / WeakRef / Tempfile) must not
    # mint a Handle: its public method_missing binds and calls the private
    # method the guest names (Kernel#system), which the dispatch floor reads
    # as ordinary Service behaviour, so the mint point refuses it.
    resp = dispatch("a_forwarder")

    assert_equal false, resp.ok?,
                 "a Service returning a transparent forwarder must not mint a Handle onto its forwarding surface"
    assert_equal "runtime", resp.payload.type,
                 "a forwarder return must surface as the runtime fault"
    assert_equal 0, @handler.size,
                 "a forwarder return must allocate no Handle entry"
  end

  # @behavior T-123
  def test_proc_return_is_still_wrapped_as_handle
    # A Proc stays wrappable (its reflective #binding is blocked at dispatch
    # on the resulting Handle); only Binding / Method / UnboundMethod are refused.
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
                 "##{meth} gadget return must surface as the runtime fault"
    refute_match(/Kobako::/, resp.payload.message,
                 "the refusal of ##{meth} is kobako's own, so it must not wear the " \
                 "<class>: <message> shape a Service exception crosses in")
    assert_equal 0, @handler.size,
                 "##{meth} must allocate no Handle entry"
  end
end
