# frozen_string_literal: true

require "test_helper"

# E2E — how a bind path materializes into guest proxies through
# real mruby. A single-segment path binds a top-level constant; a
# multi-segment path nests the leaf under a module per prefix segment. The
# dispatch value path itself lives in test_dispatch_args.rb.
class TestE2EBindPaths < Minitest::Test
  include E2eGuestHelper

  # @behavior SV-006
  def test_single_segment_path_binds_a_top_level_service
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Clock", -> { 42 })

    result = sandbox.eval("Clock.call").value

    assert_equal 42, result,
                 "a single-segment bind path must materialize as a top-level guest proxy"
  end

  # @behavior SV-007
  # Three segments exercise the intermediate module walk.
  def test_deeply_nested_path_binds_under_a_module_chain
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("MyService::Nested::KV", ->(city) { "@#{city}" })

    result = sandbox.eval('MyService::Nested::KV.call("paris")').value

    assert_equal "@paris", result,
                 "a 3-segment bind path must nest the leaf under MyService::Nested"
  end

  # @behavior SV-008
  # Both leaves are called, since a shared namespace is only proven by each
  # leaf still dispatching to its own Service.
  def test_paths_sharing_a_namespace_bind_side_by_side
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Shop::Cart", -> { "cart" })
    sandbox.bind("Shop::Orders", -> { "orders" })

    result = sandbox.eval("[Shop::Cart.call, Shop::Orders.call]").value

    assert_equal %w[cart orders], result,
                 "two bind paths under one namespace must each materialize as a distinct guest " \
                 "proxy under the shared module"
  end

  # @behavior SV-048
  # The guest's constants are materialized from its own Sandbox's bindings,
  # so a second Sandbox over the same artifact has no name to reach.
  def test_a_service_bound_on_one_sandbox_is_unreachable_from_another
    Kobako::Sandbox.new(wasm_path: REAL_WASM).bind("Clock", -> { 42 })
    other = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = other.eval("begin; Clock.call; rescue NameError; :unreachable; end").value

    assert_equal :unreachable, result,
                 "a Service bound on one Sandbox must not be reachable from guest code running " \
                 "through #eval on another Sandbox"
  end

  # @behavior SV-009
  # A namespace is the whole prefix, not the root segment. The two shapes a
  # single-segment prefix cannot tell apart are both here — leaves sharing a
  # two-segment namespace, and sibling namespaces meeting only at their root
  # — and each leaf is reached by its full path, so a leaf that nested under
  # the wrong module raises NameError instead of answering.
  def test_namespaces_are_keyed_by_the_whole_prefix
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Shop::Eu::Cart", -> { "eu-cart" })
    sandbox.bind("Shop::Eu::Orders", -> { "eu-orders" })
    sandbox.bind("Shop::Us::Cart", -> { "us-cart" })

    result = sandbox.eval("[Shop::Eu::Cart.call, Shop::Eu::Orders.call, Shop::Us::Cart.call]").value

    assert_equal %w[eu-cart eu-orders us-cart], result,
                 "bind paths that share a two-segment namespace and ones that share only their root " \
                 "must each materialize under the namespace their own prefix spells"
  end

  # @behavior SV-031
  # The refusal names the invocation that closed registration, so it does not
  # read as an ordinary path collision.
  def test_bind_after_the_first_invocation_is_refused
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Early::A", -> { :a })
    sandbox.eval("1")

    err = assert_raises(ArgumentError) { sandbox.bind("Late::B", -> { :b }) }
    assert_match(/after first Sandbox invocation/, err.message,
                 "a bind through Sandbox#bind after the first #eval must be refused naming that invocation")
  end
end
