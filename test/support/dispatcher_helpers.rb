# frozen_string_literal: true

# Shared scaffolding for the Transport::Dispatcher unit classes under
# test/transport/ — fast and deterministic, exercising the Dispatcher /
# Wire integration directly without a live Sandbox. Live-Sandbox elevation
# of these paths lives in +test/e2e/+ via real mruby.
module DispatcherHelpers
  # Stub +yield_to_guest+ lambda for tests that never trip a guest
  # block. Dispatch only builds the Yielder when +block_given+ is
  # true on the wire, so this lambda is never invoked by the paths
  # exercised below; raising on call surfaces an accidental yield-path
  # regression instead of silently returning an empty response.
  NO_YIELD = ->(_) { raise "unexpected yield in dispatch-only test" }

  def setup
    @handler = Kobako::Catalog::Handles.new
    @registry = Kobako::Catalog::Namespaces.new(handler: @handler)
  end

  # Drive the Dispatcher directly with the configured registry / handler
  # and the +NO_YIELD+ stub. Mirrors the closure +Sandbox#initialize+
  # installs on the Runtime via +Runtime#on_dispatch=+ (docs/behavior/dispatch.md
  # B-12) so these unit tests exercise the same entry point as the live
  # ext callback.
  def dispatch(bytes, server: @registry, handler: @handler)
    Kobako::Transport::Dispatcher.dispatch(bytes, server, handler, NO_YIELD)
  end

  # Encode a Request for +target+ (a constant name String or a
  # +Kobako::Handle+ — both ride the +target+ slot unchanged).
  def encode_request(target, method, args, kwargs)
    Kobako::Transport::Request.new(target: target, method_name: method, args: args, kwargs: kwargs).encode
  end

  def decode_response(bytes)
    Kobako::Transport::Response.decode(bytes)
  end

  # Allocate +obj+ in the test's own Catalog::Handles and return the id —
  # the host side of every Handle the guest could legitimately hold.
  def alloc_id(obj)
    @handler.alloc(obj).id
  end

  # Round-trip a Handle-target Request through the dispatcher: encode,
  # dispatch, decode — the wire shape a guest emits for B-17 chaining.
  def dispatch_handle_target(id, method, args = [], kwargs = {}, **dispatch_opts)
    req = encode_request(Kobako::Handle.restore(id), method, args, kwargs)
    decode_response(dispatch(req, **dispatch_opts))
  end
end
