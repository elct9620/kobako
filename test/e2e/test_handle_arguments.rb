# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — SPEC.md B-16: a Capability Handle the guest received earlier
# in the invocation, passed back to a Service as a dispatch argument, resolves
# on the host to the original object before the method runs. The positional,
# keyword-value, and mixed shapes are covered because a real project saw the
# Handle arrive as a string in both the args and the kwargs positions.
class TestE2EHandleArguments < Minitest::Test
  include E2eGuestHelper

  # Source returns this fixed instance so a test can pin identity, not equality.
  class Greeter
    def initialize(name) = (@name = name)
    def greet = "hi,#{@name}"
  end

  # Captures the positional args and kwargs a Service dispatch delivered, so the
  # test can assert a Handle argument arrived as the original host object.
  class Recorder
    attr_reader :args, :kwargs

    def take(*args, **kwargs)
      @args = args
      @kwargs = kwargs
      :ok
    end
  end

  # Wire Source (returns +greeter+) + Sink (a Recorder), run +code+, and return
  # the Recorder so the three argument-shape tests share one setup.
  def record_handle_argument(greeter, code)
    recorder = Recorder.new
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Source).bind(:Get, -> { greeter })
    sandbox.define(:Sink).bind(:Take, recorder)
    sandbox.eval(code)
    recorder
  end

  def test_b16_handle_as_positional_argument_resolves_to_host_object
    greeter = Greeter.new("Bob")
    recorder = record_handle_argument(greeter, "h = Source::Get.call; Sink::Take.take(h)")

    assert_same greeter, recorder.args[0],
                "B-16: a Handle passed as a positional argument must reach the Service method " \
                "as the original host object, never a Kobako::Handle"
  end

  def test_b16_handle_as_keyword_argument_resolves_to_host_object
    greeter = Greeter.new("Bob")
    recorder = record_handle_argument(greeter, "h = Source::Get.call; Sink::Take.take(cred: h)")

    assert_same greeter, recorder.kwargs[:cred],
                "B-16: a Handle passed as a keyword-argument value must reach the Service method " \
                "as the original host object, never a Kobako::Handle"
  end

  def test_b16_handle_as_mixed_positional_and_keyword_arguments_resolves_to_host_object
    greeter = Greeter.new("Bob")
    recorder = record_handle_argument(greeter, "h = Source::Get.call; Sink::Take.take(h, cred: h)")

    assert_same greeter, recorder.args[0],
                "B-16: in a mixed call the positional Handle must resolve to the host object"
    assert_same greeter, recorder.kwargs[:cred],
                "B-16: in a mixed call the keyword Handle value must resolve to the host object"
  end
end
