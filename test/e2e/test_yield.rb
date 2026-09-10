# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the block / yield round-trip through real mruby: a guest
# call site supplying a block
# surfaces as a non-nil +&block+ on the host Service method, and each
# +yield+ / +block.call+ is a synchronous round-trip into the guest via
# +__kobako_yield_to_block+, returning the block result (tag 0x01), a
# +break+ value (tag 0x02), or an error (tag 0x04) to the Service's yield
# site. The break / return unwind discrimination lives in
# test_yield_unwind.rb, and the error arm across its three shapes in
# test_yield_block_failure.rb, test_yield_block_spent.rb, and
# test_yield_value_refusal.rb.
class TestE2EYield < Minitest::Test
  include E2eGuestHelper

  # @behavior T-083
  def test_block_given_reaches_host_when_guest_supplies_block
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    observed = []
    sandbox.bind("Probe::Sees", ->(*, &block) { observed << !block.nil? })

    sandbox.eval("Probe::Sees.call { |x| x }").value

    assert_equal [true], observed,
                 "guest call site supplying a block must surface as " \
                 "non-nil &block on the host Service method"
  end

  # @behavior T-084
  def test_no_block_means_block_given_false_on_host
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    observed = []
    sandbox.bind("Probe::Sees", ->(*, &block) { observed << !block.nil? })

    sandbox.eval("Probe::Sees.call").value

    assert_equal [false], observed,
                 "guest call without a block leaves &block nil"
  end

  # @behavior T-085
  def test_single_yield_returns_block_value_to_service
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::OnceX", ->(x, &blk) { blk.call(x) })

    result = sandbox.eval("Probe::OnceX.call(21) { |x| x * 2 }").value

    assert_equal 42, result,
                 "a Service method's yield observes the block's " \
                 "last-expression value as the +yield+ expression's value"
  end

  # @behavior T-086 T-189
  def test_multi_yield_runs_block_once_per_iteration
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::MapEach", ->(items, &blk) { items.map(&blk) })

    result = sandbox.eval("Probe::MapEach.call([1, 2, 3]) { |x| x * 10 }").value

    assert_equal [10, 20, 30], result,
                 "each Service yield is an independent round-trip; " \
                 "the block runs once per iteration and the value flows back"
  end

  # @behavior T-087
  def test_block_body_dispatches_to_another_binding
    # The block body itself issues a second guest→host dispatch — to a
    # different binding that takes no block — while the first Service's
    # yield is mid-flight. This is the load-bearing shape of a `step` /
    # `with` capability that runs other capabilities inside its block:
    # the inner dispatch re-enters host dispatch beneath the active
    # Yielder, its result flows back into the block, and the block's
    # value reaches the outer Service's yield site. The break-carrying
    # nested variant lives in test_yield_unwind.rb.
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("A::Step", ->(name:, &blk) { "A[#{name}]:#{blk.call}" })
    sandbox.bind("B::Fetch", ->(key) { "fetched:#{key}" })

    result = sandbox.eval("A::Step.call(name: 'outer') { B::Fetch.call('k1') }").value

    assert_equal "A[outer]:fetched:k1", result,
                 "a guest block may dispatch to another no-block binding " \
                 "mid-yield; the inner result must flow back into the block and " \
                 "on to the outer Service's yield site"
  end

  # @behavior T-135 T-203
  # Coercing the answer to a String would hand the Service a plausible
  # value in place of one that never crossed, so the round-trip reports
  # the refusal at the yield site instead.
  def test_block_returns_unrepresentable_value_raises_at_yield_site
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::OnceX", ->(x, &blk) { blk.call(x) })

    # A bare Object has no wire representation, so the round-trip answers with
    # a 0x04 error and the Service's block.call raises at the yield site.
    err = assert_raises(Kobako::ServiceError) do
      sandbox.eval("Probe::OnceX.call(1) { |_x| Object.new }")
    end

    assert_match(/not a supported sandbox value type/, err.message,
                 "a guest block returning a value of an unsupported type " \
                 "must surface as a 0x04 error at the yield site, not a coerced String")
  end

  # @behavior T-091 T-203
  # The break arm returns to the guest rather than to host code, so it
  # needs its own witness that the value is refused rather than coerced
  # on the way out.
  def test_break_with_unrepresentable_value_raises_at_yield_site
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Each", ->(items, &blk) { items.each(&blk) })

    # `break Object.new` is a real break, but its value cannot ride the 0x02
    # break tag, so the guest emits a 0x04 error instead of coercing it.
    err = assert_raises(Kobako::ServiceError) do
      sandbox.eval("Probe::Each.call([1, 2, 3]) { |_x| break Object.new }")
    end

    assert_match(/not a supported sandbox value type/, err.message,
                 "a break value of an unsupported type must surface as a " \
                 "0x04 error, not unwind the Service method with a coerced String")
  end

  # @behavior T-088
  def test_service_with_block_that_never_yields_runs_clean
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Ignores", ->(*, &_blk) { :ok })

    result = sandbox.eval("Probe::Ignores.call { raise 'never runs' }").value

    assert_equal :ok, result,
                 "a Service that receives a block but never invokes " \
                 "it must complete normally — the block body never executes"
  end
end
