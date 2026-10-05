# frozen_string_literal: true

require "test_helper"

# E2E — an object whose predicate narrows its surface to nothing, through
# real mruby. Narrowing decides which names the guest may call, not whether
# the guest may hold the object, so the reference still travels: into the
# guest, back out as an argument, and back out as the answer.
class TestE2ENarrowedReference < Minitest::Test
  include E2eGuestHelper

  # Exposes no name at all.
  class Sealed
    def read = "secret"
    def respond_to_guest?(_name) = false
  end

  # @behavior T-238
  def test_a_reference_narrowed_to_nothing_still_travels
    sealed = Sealed.new
    received = []
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Vault::Open", -> { sealed })
    sandbox.bind("Vault::Keep", ->(item) { received << item })

    answer = sandbox.eval("held = Vault::Open.call; Vault::Keep.call(held); held").value

    assert_equal [sealed, [sealed]], [answer, received],
                 "an object narrowed to nothing must still be held by the guest, arrive as itself " \
                 "when passed as a dispatch argument, and return as itself through #eval"
  end

  # @behavior T-239
  def test_a_narrowed_name_left_unrescued_fails_as_a_service_failure
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Vault::Open", -> { Sealed.new })

    assert_raises(Kobako::ServiceError,
                  "a narrowed name called and left unrescued through #eval must fail as a Service failure") do
      sandbox.eval("Vault::Open.call.read")
    end
  end
end
