# frozen_string_literal: true

# Shared scaffolding for the Transport::Dispatcher unit classes under
# test/unit/transport/ — fast and deterministic, exercising the
# Dispatcher / payload codec directly without a live Sandbox.
# Live-Sandbox elevation of these paths lives in +test/e2e/+ via real
# mruby.
module DispatcherHelpers
  # Stub +yield_to_guest+ lambda for tests that never trip a guest
  # block. Dispatch only builds the Yielder when +block_given+ is
  # true on the wire, so this lambda is never invoked by the paths
  # exercised below; raising on call surfaces an accidental yield-path
  # regression instead of silently returning an empty response.
  NO_YIELD = ->(_) { raise "unexpected yield in dispatch-only test" }

  # A Reply read back in the vocabulary these tests are written in: the
  # arm the native side tagged, plus what that arm carries. The ok arm
  # carries a codec-decoded value; the fault arm carries the Fault's own
  # two fields, which the envelope types rather than the codec.
  Answer = Struct.new(:ok, :payload) do
    def ok? = ok
    def error? = !ok
  end

  # The fault arm's fields, named as the wire contract names them so the
  # assertions read against the contract rather than a tuple position.
  Fault = Struct.new(:type, :message)

  # Build the Call the native side would hand Ruby, for the test classes
  # that drive the Dispatcher without including this module.
  def self.call_for(target, method, args = [], kwargs = {}, block_given: false)
    Kobako::Transport::Call.new(
      target: target,
      method_name: method,
      block_given: block_given,
      payload: Kobako::Payload::Arguments.new(args: args, kwargs: kwargs).encode
    )
  end

  # Reify the Dispatcher's +[ok, body, kind]+ answer into the arm plus
  # what it carries: a decoded value on the ok arm, a Fault on the other.
  def self.reify(answer)
    ok, body, kind = answer
    return Answer.new(true, Kobako::Codec::Decoder.decode(body)) if ok

    Answer.new(false, Fault.new(kind, body))
  end

  def setup
    @handler = Kobako::Catalog::Handles.new
    @registry = Kobako::Catalog::Services.new
  end

  # Drive the Dispatcher directly with the configured registry / handler
  # and the +NO_YIELD+ stub. Mirrors the per-invocation dispatch +Proc+
  # +Sandbox+ hands to +Runtime#eval+ / +#run+ (docs/behavior/dispatch.md
  # B-12) so these unit tests exercise the same entry point as the live
  # ext callback.
  def dispatch(call, server: @registry, handler: @handler)
    Kobako::Transport::Dispatcher.dispatch(call, server, handler, NO_YIELD)
  end

  # Instance-side shorthands for the two module functions above, so an
  # including test class reads without the namespace. +target+ is a
  # constant-path String or a Handle id Integer — the two forms the core
  # envelope's +kind+ tag already discriminated.
  def build_call(target, method, args = [], kwargs = {}, block_given: false)
    DispatcherHelpers.call_for(target, method, args, kwargs, block_given: block_given)
  end

  def reify(answer) = DispatcherHelpers.reify(answer)

  # Allocate +obj+ in the test's own Catalog::Handles and return the id —
  # the host side of every Handle the guest could legitimately hold.
  def alloc_id(obj)
    @handler.alloc(obj).id
  end

  # Round-trip a Handle-target Call through the dispatcher: build,
  # dispatch, reify — the shape a guest emits for B-17 chaining.
  def dispatch_handle_target(id, method, args = [], kwargs = {}, **dispatch_opts)
    reify(dispatch(build_call(id, method, args, kwargs), **dispatch_opts))
  end
end
