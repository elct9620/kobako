# frozen_string_literal: true

require "test_helper"

# E2E — how a guest block takes what a yield hands it, through real mruby.
# The block is the guest's own, so it keeps the guest's argument rules: a
# block is lenient about the count, a lambda is strict, and `next` answers
# the yield the way falling off the end does.
class TestE2EYieldArity < Minitest::Test
  include E2eGuestHelper

  # @behavior T-231
  def test_a_block_drops_extra_arguments_and_leaves_missing_ones_nil
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Yields", ->(*args, &blk) { blk.call(*args) })

    result = sandbox.eval(<<~RUBY).value
      [Probe::Yields.call(1, 2, 3) { |a, b| [a, b] },
       Probe::Yields.call(1) { |a, b| [a, b] }]
    RUBY

    assert_equal [[1, 2], [1, nil]], result,
                 "a block yielded more arguments than it declares must drop the extras, and one " \
                 "yielded fewer must receive nil for the missing ones"
  end

  # @behavior T-232
  def test_a_lambda_block_refuses_a_yield_of_the_wrong_count
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Yields", lambda do |*args, &blk|
      blk.call(*args)
    rescue Kobako::BlockError => e
      e.klass
    end)

    result = sandbox.eval("Probe::Yields.call(1, 2, &->(a) { a })").value

    assert_equal "ArgumentError", result,
                 "a lambda passed as the block must refuse a yield whose argument count it does " \
                 "not accept, as a guest ArgumentError at the Service's yield"
  end

  # @behavior T-233
  def test_next_answers_the_yield_as_falling_through_does
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Yields", ->(&blk) { [blk.call(true), blk.call(false)] })

    result = sandbox.eval("Probe::Yields.call { |early| next :by_next if early; :by_end }").value

    assert_equal %i[by_next by_end], result,
                 "a block ending with next must answer the yield with that value, as falling through does"
  end
end
