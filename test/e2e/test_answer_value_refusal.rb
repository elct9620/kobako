# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — a Service answer the host cannot write. The Service returns
# whatever host object it holds and the boundary converts it, so a value
# nesting past the wire's bound — one nesting without end included — fails
# while the dispatch is still being answered, and one right at the bound
# still crosses. The outbound yield half — what the Service sends into the
# block — lives in test_yield_value_refusal.rb.
class TestE2EAnswerValueRefusal < Minitest::Test
  include E2eGuestHelper

  CALL_ONCE = "Probe::Answer.call"

  # The Service ran and produced something; only it can change what that is.
  # A trap would tell the Host App to discard the Sandbox for a failure that
  # is neither the guest's nor the runtime's.
  # @behavior CD-018
  def test_an_unwritable_answer_reaches_the_host_app_as_a_service_failure
    error = assert_raises(Kobako::ServiceError) { cyclic_sandbox.eval(CALL_ONCE) }

    assert_instance_of Kobako::ServiceError, error,
                       "a Service answer the host cannot write through #eval must reach the Host " \
                       "App as a Service failure, not as an exchange that produced no Service " \
                       "outcome and not as a trap"
  end

  # @behavior CD-019
  def test_an_unwritable_answer_answers_in_kobakos_own_wording
    error = assert_raises(Kobako::ServiceError) { cyclic_sandbox.eval(CALL_ONCE) }

    refute_match(/Kobako::/, error.message,
                 "the refusal is kobako's own, so it must not wear the <class>: <message> " \
                 "shape a Service exception crosses in")
  end

  # The guest may rescue it like any other Service failure, which is what
  # separates it from a trap: the invocation goes on to produce a value.
  RESCUING = <<~RUBY
    begin
      Probe::Answer.call
      :unreached
    rescue
      :rescued
    end
  RUBY

  # @behavior CD-020
  def test_the_guest_may_rescue_an_unwritable_answer_and_carry_on
    seen = cyclic_sandbox.eval(RESCUING).value

    assert_equal :rescued, seen,
                 "a guest rescuing a Service answer the host could not write must go on to " \
                 "finish the invocation, so the failure is the dispatch's rather than the run's"
  end

  # The refusal's pairing: a bound placed one level too shallow would still
  # refuse everything past it, so only an answer right at the bound shows
  # that the bound sits where the wire says.
  MEASURE_DEPTH = <<~RUBY
    value = Probe::Answer.call
    depth = 0
    while value.is_a?(Array) && !value.empty?
      value = value[0]
      depth += 1
    end
    depth
  RUBY

  # @behavior CD-037
  def test_an_answer_at_the_deepest_nesting_reaches_the_guest_unchanged
    depth = answering(nested(Kobako::Codec::MAX_NESTING_DEPTH)).eval(MEASURE_DEPTH).value

    assert_equal Kobako::Codec::MAX_NESTING_DEPTH, depth,
                 "a Service answer nested to the wire bound through #eval must reach the guest " \
                 "nested to that depth"
  end

  # @behavior CD-038
  def test_an_answer_one_level_past_the_bound_is_the_services_failure
    error = assert_raises(Kobako::ServiceError) do
      answering(nested(Kobako::Codec::MAX_NESTING_DEPTH + 1)).eval(CALL_ONCE)
    end

    assert_match(/could not write the Service's answer/, error.message,
                 "a Service answer nested one level past the wire bound through #eval must reach " \
                 "the Host App as the Service's failure to be written, not travel to the guest")
  end

  # A map's keys are measured apart from its values, so the depth must also be
  # found when a key is what carries it.
  # @behavior CD-038
  def test_an_answer_whose_key_nests_past_the_bound_is_the_services_failure
    error = assert_raises(Kobako::ServiceError) do
      answering({ nested(Kobako::Codec::MAX_NESTING_DEPTH) => 1 }).eval(CALL_ONCE)
    end

    assert_match(/could not write the Service's answer/, error.message,
                 "a Service answer whose map key nests one level past the wire bound through " \
                 "#eval must reach the Host App as the Service's failure to be written")
  end

  # The packer walks a map through frames that carry no stack guard, so a map
  # holding itself that reached it would end the host process; only a refusal
  # made before the write can answer it.
  # @behavior CD-039
  def test_an_answer_that_is_a_map_holding_itself_is_the_services_failure
    cyclic_map = {}.tap { |map| map["self"] = map }

    error = assert_raises(Kobako::ServiceError) { answering(cyclic_map).eval(CALL_ONCE) }

    assert_match(/could not write the Service's answer/, error.message,
                 "a Service answering a map that holds itself through #eval must reach the Host " \
                 "App as the Service's failure to be written")
  end

  private

  # A Service returning a self-referential Array — representable in type, but
  # nesting without bound, so the host cannot write it.
  def cyclic_sandbox
    answering([].tap { |a| a << a })
  end

  def answering(value)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Answer", -> { value })
    sandbox
  end

  # A list nesting +depth+ levels around an empty one.
  def nested(depth)
    (1..depth).reduce([]) { |inner, _| [inner] }
  end
end
