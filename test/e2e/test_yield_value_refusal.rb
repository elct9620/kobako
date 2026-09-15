# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — a yield argument the host cannot write. The
# Service yields whatever host object it holds and the boundary converts
# it, so a value outside the wire type set, or nesting past the wire's
# bound, fails at the yield site before the guest is re-entered, and
# arguments right at the bound still reach the block. The inbound half —
# what the block sends back — lives in test_yield_block_failure.rb.
class TestE2EYieldValueRefusal < Minitest::Test
  include E2eGuestHelper

  YIELD_ONCE = "Probe::Yields.call { |x| x }"

  # @behavior T-154
  def test_a_service_rescuing_its_own_yield_refusal_answers_normally
    seen = rescuing_sandbox.eval(YIELD_ONCE).value

    assert_equal :recovered, seen,
                 "a Service rescuing its own yield refusal through #eval must answer normally, " \
                 "so the refusal is the Service's to handle rather than the invocation's end"
  end

  # @behavior T-155
  def test_the_refusal_names_the_position_rather_than_a_codec_class
    rescuing_sandbox.eval(YIELD_ONCE)

    assert_match(/Service yielded a value the block cannot receive/, @caught.message,
                 "a yield argument outside the wire type set must reach the Service naming the " \
                 "position it failed at, not a codec class the Service never named")
  end

  # The block never runs, so nothing the guest could have done to the value
  # is observable — the round-trip never left the host.
  BLOCK_RAN_PROBE = <<~RUBY
    ran = false
    Probe::Yields.call { |_| ran = true }
    ran
  RUBY

  # @behavior T-156
  def test_the_block_never_runs
    seen = rescuing_sandbox.eval(BLOCK_RAN_PROBE).value

    assert_equal false, seen,
                 "a yield argument the host cannot write through #eval must fail before the " \
                 "guest is re-entered, so the block body never executes"
  end

  # Unrescued it is the Service failing, since the Service is the only side
  # that can change what it yields.
  # @behavior T-157
  def test_an_unrescued_yield_refusal_reaches_the_host_app_as_a_service_failure
    err = assert_raises(Kobako::ServiceError) { propagating_sandbox.eval(YIELD_ONCE) }

    assert_instance_of Kobako::ServiceError, err,
                       "an unrescued yield refusal through #eval must reach the Host App as a " \
                       "Service failure, not as an exchange that produced no Service outcome"
  end

  # @behavior T-158
  def test_an_unrescued_yield_refusal_answers_in_kobakos_own_wording
    err = assert_raises(Kobako::ServiceError) { propagating_sandbox.eval(YIELD_ONCE) }

    refute_match(/Kobako::/, err.message,
                 "the refusal is kobako's own, so it must not wear the <class>: <message> " \
                 "shape a Service exception crosses in")
  end

  # The site refuses unlike values: one the wire has no type for, one it
  # cannot reach the end of, and one nesting past its bound. Each is the
  # Service's own outbound value, so each answers here rather than
  # travelling any further.
  # @behavior T-159
  def test_a_yield_argument_that_nests_without_bound_refuses_at_the_same_site
    seen = cyclic_yield_sandbox.eval(YIELD_ONCE).value

    assert_equal :recovered, seen,
                 "a yield argument nesting without bound must reach the Service at its own " \
                 "yield site, the way a value outside the wire type set does"
  end

  # The yielded arguments travel as one list, so a value right at the wire
  # bound already nests one level past it once it is carried.
  # @behavior T-216
  def test_yield_arguments_one_level_past_the_bound_refuse_at_the_yield_site
    at_bound = (1..Kobako::Codec::MAX_NESTING_DEPTH).reduce([]) { |inner, _| [inner] }

    seen = rescuing_yield_of(at_bound).eval(YIELD_ONCE).value

    assert_equal :recovered, seen,
                 "yield arguments nesting one level past the wire bound through #eval must " \
                 "reach the Service at its own yield site, the way a value nesting without end does"
  end

  # The block measures what arrived, so a yield bound placed one level too
  # shallow — which the answer path's witness cannot see — fails here.
  MEASURE_IN_BLOCK = <<~RUBY
    Probe::Yields.call do |value|
      depth = 0
      while value.is_a?(Array) && !value.empty?
        value = value[0]
        depth += 1
      end
      depth
    end
  RUBY

  # @behavior T-217
  def test_yield_arguments_at_the_bound_reach_the_block
    within_bound = (1...Kobako::Codec::MAX_NESTING_DEPTH).reduce([]) { |inner, _| [inner] }

    depth = rescuing_yield_of(within_bound).eval(MEASURE_IN_BLOCK).value

    assert_equal Kobako::Codec::MAX_NESTING_DEPTH - 1, depth,
                 "yield arguments nested to the wire bound through #eval must reach the block " \
                 "nested to that depth"
  end

  private

  def rescuing_yield_of(value)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Yields", lambda do |&blk|
      blk.call(value)
    rescue Kobako::YieldValueError
      :recovered
    end)
    sandbox
  end

  # A Service yielding a value with no wire representation, rescuing the
  # refusal and recording it so a test can read what it said.
  def rescuing_sandbox
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Yields", lambda do |&blk|
      blk.call(Object.new)
    rescue Kobako::YieldValueError => e
      @caught = e
      :recovered
    end)
    sandbox
  end

  # The same Service, letting the refusal reach the dispatch boundary.
  def propagating_sandbox
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Yields", ->(&blk) { blk.call(Object.new) })
    sandbox
  end

  # The same Service again, yielding a value the wire has a type for but no
  # end to.
  def cyclic_yield_sandbox
    rescuing_yield_of([].tap { |a| a << a })
  end
end
