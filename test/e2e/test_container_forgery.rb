# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — a guest value whose class carries a container's name
# without being one, across the two guest→host value paths: the outcome
# (#eval return) and a dispatch argument.
#
# An anonymous class takes the name of the constant it is assigned to, so a
# guest can bind +Array+ or +Hash+ to a class of its own and hand back an
# instance of it. The encoder reads a value's wire form from its class name,
# so the name alone once decided the value was a container — an object's
# instance-variable table read as a list's slots. The regression these guard
# against is silent at its mildest: an object carrying no instance variable
# crossed as an empty list, leaving nothing for a caller to notice.
#
# One kind per position rather than both at both: the two arms share the one
# converter, so a list at the outcome and a map at a dispatch argument reach
# each arm once and each path once.
class TestE2EContainerForgery < Minitest::Test
  include E2eGuestHelper

  # A class of the guest's own bound to +Array+, so its instances answer
  # "Array" to every name-based reading while being ordinary objects.
  FORGED_LIST_ANSWER = <<~RUBY
    Object.const_set(:Array, Class.new)
    Array.new
  RUBY

  FORGED_MAP_ARGUMENT = <<~RUBY
    Object.const_set(:Hash, Class.new)
    Probe::Sink.call(Hash.new)
  RUBY

  # @behavior CD-041
  def test_outcome_value_bearing_a_lists_name_is_refused
    err = assert_raises(Kobako::SandboxError) do
      Kobako::Sandbox.new(wasm_path: REAL_WASM).eval(FORGED_LIST_ANSWER)
    end

    assert_match(/type Array is not a supported/, err.message,
                 "an object whose class carries a list's name without being one through #eval " \
                 "must be refused as an unrepresentable return value, not read as a list")
  end

  # The Service is asked whether it was reached because the refusal's own
  # class cannot tell a value stopped at the call site from one the Service
  # received and rejected.
  # @behavior CD-042
  def test_dispatch_argument_bearing_a_maps_name_is_refused
    reached = false
    sandbox = sink_sandbox { reached = true }

    err = assert_raises(Kobako::SandboxError) { sandbox.eval(FORGED_MAP_ARGUMENT) }

    assert_equal "TypeError", err.klass,
                 "an object whose class carries a map's name without being one through a " \
                 "dispatch argument must be refused as the script's own type error"
    refute reached,
           "an object whose class carries a map's name without being one through a dispatch " \
           "argument must not reach the Service"
  end

  private

  # A Sandbox whose one Service records that it was reached at all.
  def sink_sandbox(&recorder)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Sink", ->(_value) { recorder.call })
    sandbox
  end
end
