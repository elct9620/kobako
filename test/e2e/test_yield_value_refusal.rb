# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — a yield argument the host cannot write (E-57). The
# Service yields whatever host object it holds and the boundary converts
# it, so a value outside the wire type set fails at the yield site before
# the guest is re-entered. The inbound half — what the block sends back —
# lives in test_yield_block_failure.rb.
class TestE2EYieldValueRefusal < Minitest::Test
  include E2eGuestHelper
  include StackQuarantine

  YIELD_ONCE = "Probe::Yields.call { |x| x }"

  def test_e57_a_service_rescuing_its_own_yield_refusal_answers_normally
    seen = rescuing_sandbox.eval(YIELD_ONCE).value

    assert_equal :recovered, seen,
                 "a Service rescuing its own yield refusal through #eval must answer normally, " \
                 "so the refusal is the Service's to handle rather than the invocation's end"
  end

  def test_e57_the_refusal_names_the_position_rather_than_a_codec_class
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

  def test_e57_the_block_never_runs
    seen = rescuing_sandbox.eval(BLOCK_RAN_PROBE).value

    assert_equal false, seen,
                 "a yield argument the host cannot write through #eval must fail before the " \
                 "guest is re-entered, so the block body never executes"
  end

  # Unrescued it is the Service failing, since the Service is the only side
  # that can change what it yields.
  def test_e57_an_unrescued_yield_refusal_reaches_the_host_app_as_a_service_failure
    err = assert_raises(Kobako::ServiceError) { propagating_sandbox.eval(YIELD_ONCE) }

    assert_instance_of Kobako::ServiceError, err,
                       "an unrescued yield refusal through #eval must reach the Host App as a " \
                       "Service failure, not as an exchange that produced no Service outcome"
  end

  def test_e57_an_unrescued_yield_refusal_answers_in_kobakos_own_wording
    err = assert_raises(Kobako::ServiceError) { propagating_sandbox.eval(YIELD_ONCE) }

    refute_match(/Kobako::/, err.message,
                 "the refusal is kobako's own, so it must not wear the <class>: <message> " \
                 "shape a Service exception crosses in")
  end

  # The site refuses two unlike values: one the wire has no type for, and one
  # it cannot reach the end of. Both are the Service's own outbound value, so
  # both answer here rather than travelling any further. This one runs on a
  # stack of its own — refusing it costs the thread that takes it (see
  # StackQuarantine).
  def test_e57_a_yield_argument_that_nests_without_bound_refuses_at_the_same_site
    seen = in_a_spendable_stack { cyclic_yield_sandbox.eval(YIELD_ONCE).value }

    assert_equal :recovered, seen,
                 "a yield argument nesting without bound must reach the Service at its own " \
                 "yield site, the way a value outside the wire type set does"
  end

  private

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
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Yields", lambda do |&blk|
      blk.call([].tap { |a| a << a })
    rescue Kobako::YieldValueError
      :recovered
    end)
    sandbox
  end
end
