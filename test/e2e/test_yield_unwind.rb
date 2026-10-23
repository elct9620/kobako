# frozen_string_literal: true

require "test_helper"

# E2E — a block's `break` and `return` unwinding a Service as they would
# a plain method, and a lambda's `break` staying inside the lambda. The
# basic yield round-trip lives in test_yield.rb.
class TestE2EYieldUnwind < Minitest::Test
  include E2eGuestHelper

  # @behavior T-089
  def test_break_in_block_unwinds_service_to_break_value
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Each", ->(items, &blk) { items.each(&blk) })

    result = sandbox.eval("Probe::Each.call([1, 2, 3]) { |x| break :stop if x == 2 }").value

    assert_equal :stop, result,
                 "`break val` inside the guest block must terminate the " \
                 "Service method with +val+ as its effective return value"
  end

  # @behavior T-090
  def test_lambda_break_returns_value_silently
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::OnceX", ->(x, &blk) { blk.call(x) })

    # mruby treats lambda `break` as a silent normal return
    # (MRB_PROC_STRICT_P → NORMAL_RETURN, vm.c:2749) — `mrb->exc`
    # stays nil and the block evaluates to the break value via
    # tag 0x10 ok. From the Service method's view, this is
    # indistinguishable from a regular `next val` return.
    result = sandbox.eval("Probe::OnceX.call(7, &->(x) { break x * 3 })").value

    assert_equal 21, result,
                 "lambda `break val` is a silent return — the Service's " \
                 "yield observes the break value as a normal `next` outcome"
  end

  # A Service that would answer :finished shows whether it ran to its end.
  PROC_RETURN_SCRIPT = "def make_return; Probe::OnceX.call(5) { |x| return x * 2 }; :fell_through; end; make_return"

  # @behavior T-134
  def test_proc_return_leaves_the_method_around_the_call
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::OnceX", ->(x, &blk) { blk.call(x) && :finished })

    result = sandbox.eval(PROC_RETURN_SCRIPT).value

    assert_equal 10, result,
                 "a `return` from a block through Sandbox#eval must leave the method around " \
                 "the Call with its value, the Service unwound before it finished"
  end

  # The guest's BLOCK_STACK pushes / pops in strict LIFO so each yield
  # round-trip targets the correct frame, and an inner +break+ terminates
  # only the inner Service.
  NESTED_BREAK_SCRIPT = <<~RUBY
    Probe::Outer.call([1, 2]) do |a|
      inner = Probe::Inner.call([10, 20]) { |b| break :inner_stop if b == 20; b }
      [a, inner]
    end
  RUBY

  # @behavior T-092 T-188
  def test_nested_dispatch_frames_each_carry_their_own_block
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Outer", ->(items, &blk) { items.map(&blk) })
    sandbox.bind("Probe::Inner", lambda { |items, &blk|
      items.each { |x| blk.call(x) }
      :inner_done
    })

    result = sandbox.eval(NESTED_BREAK_SCRIPT).value

    # Outer iterates [1, 2]; each iteration runs Inner which iterates
    # [10, 20] and breaks on 20 with :inner_stop. Outer's block sees
    # :inner_stop for each outer iteration, so the final result is
    # the map [[1, :inner_stop], [2, :inner_stop]].
    assert_equal [[1, :inner_stop], [2, :inner_stop]], result,
                 "inner break terminates only the inner Service; the " \
                 "outer block resumes normally for each outer iteration"
  end

  # A Service method stashes its block and invokes it from a later dispatch,
  # after the originating frame has returned.
  ESCAPE_SCRIPT = "Probe::Stash.stash { :payload }; Probe::Stash.replay"

  # @behavior T-136 T-205
  # The Dispatcher's ensure block invalidates the Yielder as its frame
  # returns, so a Service that stored the block finds it switched off
  # rather than reaching into a frame that is no longer there.
  def test_escaped_yielder_invocation_raises_local_jump_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    stash_service = Class.new do
      def stash(&block) = (@blk = block)
      def replay = @blk.call
    end.new
    sandbox.bind("Probe::Stash", stash_service)

    err = assert_raises(Kobako::ServiceError) { sandbox.eval(ESCAPE_SCRIPT) }

    assert_match(/LocalJumpError/, err.message,
                 "invoking the Yielder after its dispatch frame " \
                 "returned must raise LocalJumpError host-side")
  end

  # A nested dispatch refused before reaching the host still carries a block,
  # and the guest raises out of it through mruby's longjmp, which runs no Rust
  # destructor; encoding the argument before parking the block keeps that
  # raise from stranding the block where the outer Service's next yield would
  # find it. The refusal is a +TypeError+ because the argument has no wire
  # representation.
  REFUSED_INNER_SCRIPT = <<~RUBY
    Probe::Outer.call([1, 2]) do |a|
      begin
        Probe::Inner.call(Object.new) { |b| b }
      rescue TypeError
        a * 10
      end
    end
  RUBY

  # @behavior T-093
  def test_a_refused_nested_call_leaves_the_outer_block_reachable
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Outer", ->(items, &blk) { items.map(&blk) })
    sandbox.bind("Probe::Inner", ->(items, &blk) { items.each(&blk) })

    result = sandbox.eval(REFUSED_INNER_SCRIPT).value

    assert_equal [10, 20], result,
                 "a nested call refused before it reached the host must leave " \
                 "the outer Service yielding into its own block, so every outer " \
                 "iteration still runs the block the outer call site supplied"
  end
end
