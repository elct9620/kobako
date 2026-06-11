# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — break / lambda-break / Proc-return discrimination at the
# yield boundary (docs/behavior.md B-25 / B-27 / B-28, E-21 / E-23). The
# guest yield export classifies the post-protect RBreak by comparing its
# `ci_break_index` against the pre-yield baseline: an index ≥ baseline lands
# on the yielder's frame (a real `break`, tag 0x02); an index < baseline
# aims past the yielder (a non-orphan Proc `return`) and emits tag 0x04
# LocalJumpError per E-21. The basic yield round-trip lives in
# test_yield.rb.
class TestE2EYieldUnwind < Minitest::Test
  include E2eGuestHelper

  def test_b25_break_in_block_unwinds_service_to_break_value
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Probe).bind(:Each, ->(items, &blk) { items.each(&blk) })

    result = sandbox.eval("Probe::Each.call([1, 2, 3]) { |x| break :stop if x == 2 }")

    assert_equal :stop, result,
                 "B-25: `break val` inside the guest block must terminate the " \
                 "Service method with +val+ as its effective return value"
  end

  def test_b27_lambda_break_returns_value_silently
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Probe).bind(:OnceX, ->(x, &blk) { blk.call(x) })

    # mruby treats lambda `break` as a silent normal return
    # (MRB_PROC_STRICT_P → NORMAL_RETURN, vm.c:2749) — `mrb->exc`
    # stays nil and the block evaluates to the break value via
    # tag 0x01 ok. From the Service method's view, this is
    # indistinguishable from a regular `next val` return.
    result = sandbox.eval("Probe::OnceX.call(7, &->(x) { break x * 3 })")

    assert_equal 21, result,
                 "B-27: lambda `break val` is a silent return — the Service's " \
                 "yield observes the break value as a normal `next` outcome"
  end

  # E-21: `return val` inside a guest block whose enclosing method is
  # still on the guest call stack would unwind across the host yield
  # boundary — unrepresentable on the wire. The guest classifier sees
  # an RBreak whose `ci_break_index` points deeper than the yielder's
  # frame and emits tag 0x04 LocalJumpError; the host Yielder surfaces
  # it as a Ruby exception.
  E21_RETURN_SCRIPT = "def make_return; Probe::OnceX.call(5) { |x| return x * 2 }; end; make_return"

  def test_e21_proc_return_aimed_past_yield_boundary_raises_local_jump_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Probe).bind(:OnceX, ->(x, &blk) { blk.call(x) })

    err = assert_raises(Kobako::ServiceError) { sandbox.eval(E21_RETURN_SCRIPT) }

    assert_match(/LocalJumpError/, err.message,
                 "E-21: Proc `return` aimed past the host yield boundary " \
                 "must surface as a LocalJumpError at the yield site")
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

  def test_b28_nested_dispatch_frames_each_carry_their_own_block
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Probe).bind(:Outer, ->(items, &blk) { items.map(&blk) })
    sandbox.define(:Probe).bind(:Inner, lambda { |items, &blk|
      items.each { |x| blk.call(x) }
      :inner_done
    })

    result = sandbox.eval(B28_NESTED_SCRIPT)

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

  def test_e23_escaped_yielder_invocation_raises_local_jump_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    stash_service = Class.new do
      def stash(&block) = (@blk = block)
      def replay = @blk.call
    end.new
    sandbox.define(:Probe).bind(:Stash, stash_service)

    err = assert_raises(Kobako::ServiceError) { sandbox.eval(E23_ESCAPE_SCRIPT) }

    assert_match(/LocalJumpError/, err.message,
                 "E-23: invoking the Yielder after its dispatch frame " \
                 "returned must raise LocalJumpError host-side")
  end
end
