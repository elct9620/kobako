# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the block / yield round-trip through real mruby
# (docs/behavior/yield.md B-23..B-30): a guest call site supplying a block
# surfaces as a non-nil +&block+ on the host Service method, and each
# +yield+ / +block.call+ is a synchronous round-trip into the guest via
# +__kobako_yield_to_block+, returning the block result (tag 0x01), a
# +break+ value (tag 0x02), or an error (tag 0x04) to the Service's yield
# site. The break / return unwind discrimination lives in
# test_yield_unwind.rb.
class TestE2EYield < Minitest::Test
  include E2eGuestHelper

  def test_b23_block_given_reaches_host_when_guest_supplies_block
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    observed = []
    sandbox.define(:Probe).bind(:Sees, ->(*, &block) { observed << !block.nil? })

    sandbox.eval("Probe::Sees.call { |x| x }")

    assert_equal [true], observed,
                 "B-23: guest call site supplying a block must surface as " \
                 "non-nil &block on the host Service method"
  end

  def test_b23_no_block_means_block_given_false_on_host
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    observed = []
    sandbox.define(:Probe).bind(:Sees, ->(*, &block) { observed << !block.nil? })

    sandbox.eval("Probe::Sees.call")

    assert_equal [false], observed,
                 "B-23: guest call without a block leaves &block nil"
  end

  def test_b24_single_yield_returns_block_value_to_service
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Probe).bind(:OnceX, ->(x, &blk) { blk.call(x) })

    result = sandbox.eval("Probe::OnceX.call(21) { |x| x * 2 }")

    assert_equal 42, result,
                 "B-24: a Service method's yield observes the block's " \
                 "last-expression value as the +yield+ expression's value"
  end

  def test_b29_multi_yield_runs_block_once_per_iteration
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Probe).bind(:MapEach, ->(items, &blk) { items.map(&blk) })

    result = sandbox.eval("Probe::MapEach.call([1, 2, 3]) { |x| x * 10 }")

    assert_equal [10, 20, 30], result,
                 "B-29: each Service yield is an independent round-trip; " \
                 "the block runs once per iteration and the value flows back"
  end

  def test_b24_block_raise_surfaces_to_service_yield_site
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Probe).bind(:Boom, ->(&blk) { blk.call })

    err = assert_raises(Kobako::ServiceError) do
      sandbox.eval('Probe::Boom.call { raise "from guest block" }')
    end

    assert_match(/from guest block/, err.message,
                 "B-24 Notes: an exception raised inside the guest block " \
                 "propagates back to the Service method's yield site")
  end

  def test_e22_block_returns_unrepresentable_value_raises_at_yield_site
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Probe).bind(:OnceX, ->(x, &blk) { blk.call(x) })

    # The guest block returns a bare Object — no MessagePack wire
    # representation. Per E-22 the yield round-trip emits tag 0x04 error
    # rather than coercing the value to a String, so the Service's
    # block.call raises at the yield site; unrescued, it surfaces through
    # the same path as a block exception (B-24) — Kobako::ServiceError.
    err = assert_raises(Kobako::ServiceError) do
      sandbox.eval("Probe::OnceX.call(1) { |_x| Object.new }")
    end

    assert_match(/not a supported sandbox value type/, err.message,
                 "E-22: a guest block returning a value of an unsupported type " \
                 "must surface as a 0x04 error at the yield site, not a coerced String")
  end

  def test_e22_break_with_unrepresentable_value_raises_at_yield_site
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Probe).bind(:Each, ->(items, &blk) { items.each(&blk) })

    # `break Object.new` is a real break (B-25), but the break value is
    # not a supported sandbox value type. The break value cannot ride the
    # 0x02 break tag, so the guest emits a 0x04 error instead of coercing
    # it — the Service observes an error at its yield site rather than an
    # unwind to a misleading String.
    err = assert_raises(Kobako::ServiceError) do
      sandbox.eval("Probe::Each.call([1, 2, 3]) { |_x| break Object.new }")
    end

    assert_match(/not a supported sandbox value type/, err.message,
                 "E-22: a break value of an unsupported type must surface as a " \
                 "0x04 error, not unwind the Service method with a coerced String")
  end

  def test_b30_service_with_block_that_never_yields_runs_clean
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Probe).bind(:Ignores, ->(*, &_blk) { :ok })

    result = sandbox.eval("Probe::Ignores.call { raise 'never runs' }")

    assert_equal :ok, result,
                 "B-30: a Service that receives a block but never invokes " \
                 "it must complete normally — the block body never executes"
  end
end
