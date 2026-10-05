# frozen_string_literal: true

require "test_helper"
require "support/rendezvous"

# E2E — a per-invocation provider under concurrent invocations of one
# Sandbox, through real mruby. Every invocation meets the others before
# reading its backend twice, so all of them hold a provider object at the
# same moment; one reaching another's object would see a foreign or a
# shifting identity.
class TestE2EInstallConcurrency < Minitest::Test
  include E2eGuestHelper

  INVOCATIONS = 4

  # The backend object a provider yields, carrying an identity the guest
  # can read back.
  class Backend
    def id = object_id
  end

  READ_TWICE_AROUND_THE_MEETING = "first = Store.id; Gate::Meet.call; [first, Store.id]"

  # @behavior EX-048
  def test_concurrent_invocations_each_receive_their_own_provider_object
    sandbox = concurrent_sandbox

    reads = Array.new(INVOCATIONS) { Thread.new { sandbox.eval(READ_TWICE_AROUND_THE_MEETING).value } }.map(&:value)

    assert_equal [true, INVOCATIONS], [reads.all? { |first, last| first == last }, reads.map(&:first).uniq.size],
                 "concurrent invocations on one Sandbox must each keep their own provider object, " \
                 "none seeing another's"
  end

  private

  def concurrent_sandbox
    rendezvous = Rendezvous.new(INVOCATIONS)
    backend = Kobako::Extension::Backend.new(path: "Store", provider: -> { Backend.new })
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, gvl: :release)
    sandbox.install(Kobako::Extension.new(name: :Store, source: "", backend: backend))
    sandbox.bind("Gate::Meet", -> { rendezvous.meet })
    sandbox
  end
end
