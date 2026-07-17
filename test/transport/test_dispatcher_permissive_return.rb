# frozen_string_literal: true

require "test_helper"

# Regression: a Service returning an object with a permissive method_missing
# (a builder or proxy that answers the codec's to_msgpack probe) must cross to
# the guest as a Capability Handle, not mis-encode as nil. The host codec's
# BasicObject guard makes non-wire detection an allowlist, so encode_ok wraps
# the object instead of the msgpack fallback silently dropping it.
class TestDispatchPermissiveReturn < Minitest::Test
  class Permissive
    def method_missing(_name, *) = nil
    def respond_to_missing?(_name, _include_private = false) = true
  end

  class Service
    def widget = Permissive.new
  end

  def setup
    @handler = Kobako::Catalog::Handles.new
    @services = Kobako::Catalog::Services.new(handler: @handler)
    @services.bind("Dsl::S", Service.new)
    @services.seal!
    @yield = ->(_bytes) { raise "no block" }
  end

  def dispatch(method)
    req = Kobako::Transport::Request.new(target: "Dsl::S", method_name: method, args: [])
    bytes = Kobako::Transport::Dispatcher.dispatch(req.encode, @services, @handler, @yield)
    Kobako::Transport::Response.decode(bytes)
  end

  def test_permissive_builder_return_crosses_as_handle
    resp = dispatch("widget")
    assert_equal Kobako::Transport::STATUS_OK, resp.status,
                 "a Service returning a permissive method_missing object must succeed, not mis-encode as nil"
    assert_instance_of Kobako::Handle, resp.payload,
                       "a permissive method_missing return must cross as a Capability Handle, never a nil payload"
    assert_equal 1, @handler.size,
                 "the permissive return must allocate exactly one Handle entry"
  end
end
