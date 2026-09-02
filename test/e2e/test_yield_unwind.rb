# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — break / lambda-break / Proc-return discrimination at the
# yield boundary (docs/behavior/yield.md B-25 / B-27 / B-28, E-21 / E-23). The
# guest yield export classifies the post-protect RBreak by comparing its
# `ci_break_index` against the pre-yield baseline: an index ≥ baseline lands
# on the yielder's frame (a real `break`, tag 0x02); an index < baseline
# aims past the yielder (a non-orphan Proc `return`) and emits tag 0x04
# LocalJumpError per E-21. The basic yield round-trip lives in
# test_yield.rb.
class TestE2EYieldUnwind < Minitest::Test
  include E2eGuestHelper

  # @behavior T-089
  def test_b25_break_in_block_unwinds_service_to_break_value
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Each", ->(items, &blk) { items.each(&blk) })

    result = sandbox.eval("Probe::Each.call([1, 2, 3]) { |x| break :stop if x == 2 }").value

    assert_equal :stop, result,
                 "B-25: `break val` inside the guest block must terminate the " \
                 "Service method with +val+ as its effective return value"
  end

  # @behavior T-090
  def test_b27_lambda_break_returns_value_silently
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::OnceX", ->(x, &blk) { blk.call(x) })

    # mruby treats lambda `break` as a silent normal return
    # (MRB_PROC_STRICT_P → NORMAL_RETURN, vm.c:2749) — `mrb->exc`
    # stays nil and the block evaluates to the break value via
    # tag 0x01 ok. From the Service method's view, this is
    # indistinguishable from a regular `next val` return.
    result = sandbox.eval("Probe::OnceX.call(7, &->(x) { break x * 3 })").value

    assert_equal 21, result,
                 "B-27: lambda `break val` is a silent return — the Service's " \
                 "yield observes the break value as a normal `next` outcome"
  end

  # A `return` whose target frame is still on the guest stack would have
  # to unwind across the host yield boundary, which the wire cannot carry.
  # The guest classifier sees a break index pointing deeper than the
  # yielder's frame and reports the local jump instead.
  E21_RETURN_SCRIPT = "def make_return; Probe::OnceX.call(5) { |x| return x * 2 }; end; make_return"

  # @behavior T-134
  def test_e21_proc_return_aimed_past_yield_boundary_raises_local_jump_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::OnceX", ->(x, &blk) { blk.call(x) })

    err = assert_raises(Kobako::ServiceError) { sandbox.eval(E21_RETURN_SCRIPT) }

    assert_match(/LocalJumpError/, err.message,
                 "E-21: Proc `return` aimed past the host yield boundary " \
                 "must surface as a LocalJumpError at the yield site")
  end

  # The `break` value crosses the wire like any other, so a value with no
  # representation is refused there too — the sibling of the block-return
  # rejection (E-22), on the one arm that returns to the guest rather than
  # to host code.
  # @behavior T-091
  def test_break_value_with_no_wire_representation_is_refused
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Each", ->(items, &blk) { items.each(&blk) })

    err = assert_raises(Kobako::ServiceError) do
      sandbox.eval("Probe::Each.call([1, 2]) { |x| break Object.new }")
    end

    assert_match(/TypeError: break value of type Object/, err.message,
                 "a `break` value with no wire representation through Sandbox#eval must be " \
                 "refused as a type error naming the break slot — the block-return rejection " \
                 "raises the same class, so only the slot tells the two arms apart")
  end

  # B-28: nested dispatch frames each carry their own Yielder. An
  # inner +break+ terminates only the inner Service; the outer block
  # resumes normally. The guest's BLOCK_STACK pushes / pops in strict
  # LIFO so each yield round-trip targets the correct frame.
  B28_NESTED_SCRIPT = <<~RUBY
    Probe::Outer.call([1, 2]) do |a|
      inner = Probe::Inner.call([10, 20]) { |b| break :inner_stop if b == 20; b }
      [a, inner]
    end
  RUBY

  # @behavior T-092
  def test_b28_nested_dispatch_frames_each_carry_their_own_block
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Outer", ->(items, &blk) { items.map(&blk) })
    sandbox.bind("Probe::Inner", lambda { |items, &blk|
      items.each { |x| blk.call(x) }
      :inner_done
    })

    result = sandbox.eval(B28_NESTED_SCRIPT).value

    # Outer iterates [1, 2]; each iteration runs Inner which iterates
    # [10, 20] and breaks on 20 with :inner_stop. Outer's block sees
    # :inner_stop for each outer iteration, so the final result is
    # the map [[1, :inner_stop], [2, :inner_stop]].
    assert_equal [[1, :inner_stop], [2, :inner_stop]], result,
                 "B-28: inner break terminates only the inner Service; the " \
                 "outer block resumes normally for each outer iteration"
  end

  # E-23: when a Service method stashes its block and invokes it from a
  # later dispatch (after the originating frame has returned), the host
  # Yielder raises +LocalJumpError+ — the Dispatcher's +ensure+ block
  # called +#invalidate!+, flipping the Yielder off.
  E23_ESCAPE_SCRIPT = "Probe::Stash.stash { :payload }; Probe::Stash.replay"

  # @behavior T-136
  # The Dispatcher's ensure block invalidates the Yielder as its frame
  # returns, so a Service that stored the block finds it switched off
  # rather than reaching into a frame that is no longer there.
  def test_e23_escaped_yielder_invocation_raises_local_jump_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    stash_service = Class.new do
      def stash(&block) = (@blk = block)
      def replay = @blk.call
    end.new
    sandbox.bind("Probe::Stash", stash_service)

    err = assert_raises(Kobako::ServiceError) { sandbox.eval(E23_ESCAPE_SCRIPT) }

    assert_match(/LocalJumpError/, err.message,
                 "E-23: invoking the Yielder after its dispatch frame " \
                 "returned must raise LocalJumpError host-side")
  end

  # B-28, the refused-call corner: a nested dispatch that never reaches
  # the host still carries a block, and the guest raises out of it —
  # through mruby's longjmp, which runs no Rust destructor. If the block
  # were parked before the argument was encoded, that raise would strand
  # it on the stack, and the outer Service's next yield would find the
  # stranded block instead of its own. Encoding first is what keeps the
  # park strictly inside the call that survives to make it.
  #
  # The refusal is a +TypeError+: the argument has no wire representation,
  # which is the script handing over the wrong type rather than the
  # exchange failing.
  B28_REFUSED_INNER_SCRIPT = <<~RUBY
    Probe::Outer.call([1, 2]) do |a|
      begin
        Probe::Inner.call(Object.new) { |b| b }
      rescue TypeError
        a * 10
      end
    end
  RUBY

  # @behavior T-093
  def test_b28_a_refused_nested_call_leaves_the_outer_block_reachable
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Outer", ->(items, &blk) { items.map(&blk) })
    sandbox.bind("Probe::Inner", ->(items, &blk) { items.each(&blk) })

    result = sandbox.eval(B28_REFUSED_INNER_SCRIPT).value

    assert_equal [10, 20], result,
                 "B-28: a nested call refused before it reached the host must leave " \
                 "the outer Service yielding into its own block, so every outer " \
                 "iteration still runs the block the outer call site supplied"
  end
end
