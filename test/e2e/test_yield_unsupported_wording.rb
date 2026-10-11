# frozen_string_literal: true

require "test_helper"

# The real Guest Binary serves every block, so the Service raises the
# unsupported refusal itself, standing in for a yield a guest did not
# serve. What the Host App reads is the wording the dispatch boundary gave
# that refusal on its way back through the guest.
class TestYieldUnsupportedWording < Minitest::Test
  include E2eGuestHelper

  # @behavior T-280
  def test_an_unrescued_unsupported_yield_answers_in_kobakos_own_wording
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Yields", lambda do |&_blk|
      raise Kobako::UnsupportedError, "the Sandbox runtime does not support yielding to a block"
    end)

    err = assert_raises(Kobako::ServiceError) { sandbox.eval("Probe::Yields.call { |x| x }") }

    refute_match(/Kobako::/, err.message,
                 "an unsupported yield left unrescued through #eval is kobako's own refusal, so " \
                 "it must not wear the <class>: <message> shape a Service exception crosses in")
  end
end
