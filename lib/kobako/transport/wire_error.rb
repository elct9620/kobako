# frozen_string_literal: true

require_relative "../errors"
require_relative "../transport"

module Kobako
  module Transport
    # +Kobako::SandboxError+ subclass raised when the host detects a
    # structural violation of the wire contract while decoding bytes
    # produced by the guest (a malformed Outcome envelope, a result body
    # that fails msgpack decode, a Panic envelope missing required
    # fields). Distinct from a Wasm trap (engine signalled the guest
    # runtime is unrecoverable) and from a normal sandbox-layer failure
    # (the script raised but the protocol was respected): a +WireError+
    # always indicates the guest runtime is corrupted — the only safe
    # recovery is to discard the Sandbox and start a new invocation.
    #
    # Inherits from +Kobako::SandboxError+ so a single
    # +rescue Kobako::SandboxError+ still catches it; callers that want
    # to distinguish wire-violation paths from script failures can
    # +rescue Kobako::Transport::WireError+ directly.
    class WireError < Kobako::SandboxError; end
  end
end
