# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — how a bind path materializes into guest proxies through
# real mruby (B-08). A single-segment path binds a top-level constant; a
# multi-segment path nests the leaf under a module per prefix segment. The
# dispatch value path itself lives in test_dispatch_args.rb.
class TestE2EBindPaths < Minitest::Test
  include E2eGuestHelper

  # B-08: a single-segment path installs the Service at a top-level
  # constant, so the guest reaches it with no enclosing namespace module.
  def test_single_segment_path_binds_a_top_level_service
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Clock", -> { 42 })

    result = sandbox.eval("Clock.call").value

    assert_equal 42, result,
                 "a single-segment bind path must materialize as a top-level guest proxy (B-08)"
  end

  # B-08: a three-segment path nests the leaf under a module per prefix
  # segment, exercising the intermediate module walk.
  def test_deeply_nested_path_binds_under_a_module_chain
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("MyService::Nested::KV", ->(city) { "@#{city}" })

    result = sandbox.eval('MyService::Nested::KV.call("paris")').value

    assert_equal "@paris", result,
                 "a 3-segment bind path must nest the leaf under MyService::Nested (B-08)"
  end

  # B-08: paths gathered under one namespace share that namespace's module,
  # so the second leaf lands beside the first rather than under a module of
  # its own. Both are called, since a shared namespace is only proven by
  # each leaf still dispatching to its own Service.
  def test_paths_sharing_a_namespace_bind_side_by_side
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Shop::Cart", -> { "cart" })
    sandbox.bind("Shop::Orders", -> { "orders" })

    result = sandbox.eval("[Shop::Cart.call, Shop::Orders.call]").value

    assert_equal %w[cart orders], result,
                 "two bind paths under one namespace must each materialize as a distinct guest " \
                 "proxy under the shared module (B-08)"
  end
end
