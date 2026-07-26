# frozen_string_literal: true

require_relative "../errors"

module Kobako
  module Transport
    # +Kobako::SandboxError+ subclass raised when the host detects a
    # structural violation of the wire contract while reading what the
    # guest produced — an invocation value the payload adapter cannot
    # decode. Distinct from a Wasm trap (engine signalled the guest
    # runtime is unrecoverable) and from a normal sandbox-layer failure
    # (the script raised but the protocol was respected): a
    # +Transport::Error+ always indicates the guest runtime is corrupted —
    # the only safe recovery is to discard the Sandbox and start a new
    # invocation.
    #
    # Inherits from +Kobako::SandboxError+ so a single
    # +rescue Kobako::SandboxError+ still catches it; callers that want
    # to distinguish wire-violation paths from script failures can
    # +rescue Kobako::Transport::Error+ directly.
    class Error < Kobako::SandboxError
      def initialize(message, diagnostic: nil, **)
        super(message, **)
        @diagnostic = diagnostic
      end

      # The codec fault behind this violation, appended to Ruby's own
      # rendering. A caller cannot act on the inner "Symbol payload must
      # be …" wording, so it stays out of #message; an operator
      # triaging a corrupted runtime still needs it, and
      # +detailed_message+ is where Ruby puts text of exactly that kind.
      def detailed_message(...)
        rendered = super
        return rendered if @diagnostic.nil?

        "#{rendered}\n#{@diagnostic}"
      end
    end
  end
end
