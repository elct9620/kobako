# frozen_string_literal: true

require "test_helper"

# Unit tests for what a bound callable, a bound reflective gadget, and a
# capability reference to a callable each let the guest reach. Pure Ruby —
# drives the Dispatcher directly, with no native extension.
class TestDispatchCallables < Minitest::Test
  include DispatcherHelpers

  def setup
    super
    @registry.bind("Cfg::Fn", ->(x) { x * 2 })
    @registry.bind("Cfg::Gadget", Object.new.instance_eval { binding })
  end

  def call(target, method, args = []) = reify(dispatch(build_call(target, method, args)))

  # @behavior T-224
  def test_every_name_on_the_callable_allowlist_reaches_a_bound_callable
    [["call", [21], 42], ["[]", [21], 42], ["yield", [21], 42], ["arity", [], 1], ["lambda?", [], true]]
      .each do |meth, args, want|
        resp = call("Cfg::Fn", meth, args)
        assert_equal [true, want], [resp.ok?, resp.payload],
                     "Cfg::Fn.#{meth} through guest dispatch must reach the bound callable"
      end
  end

  # A Binding bound directly is the whole of its own surface, and all of it
  # is reflection: evaluating source most of all.
  # @behavior T-223
  def test_a_reflective_gadget_bound_as_a_service_answers_none_of_its_own_methods
    %w[eval local_variable_get local_variables receiver irb].each do |meth|
      resp = call("Cfg::Gadget", meth, ["1"])
      assert_equal [false, "undefined"], [resp.ok?, resp.payload.type],
                   "Cfg::Gadget.#{meth} through guest dispatch must be refused as undefined, never run on the host"
    end
  end

  # @behavior T-225
  def test_a_reflective_name_on_a_reference_to_a_callable_is_refused_as_on_a_bound_one
    id = alloc_id(->(x) { x * 2 })
    %w[binding curry to_proc].each do |meth|
      resp = call(id, meth)
      assert_equal [false, "undefined"], [resp.ok?, resp.payload.type],
                   "#{meth} on a capability reference to a callable through guest dispatch must be refused"
    end
  end
end
