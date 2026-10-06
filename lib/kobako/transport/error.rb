# frozen_string_literal: true

require_relative "../errors"

module Kobako
  module Transport
    # +Kobako::SandboxError+ subclass raised when the host detects a
    # structural violation of the wire contract while reading what the
    # guest produced — an invocation value the payload codec cannot
    # decode. Distinct from a Wasm trap (the engine stopped the guest) and
    # from a normal sandbox-layer failure (the script raised but the
    # protocol was respected): a +Transport::Error+ says the guest broke
    # the wire contract itself.
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
      # triaging the violation still needs it, and
      # +detailed_message+ is where Ruby puts text of exactly that kind.
      def detailed_message(...)
        rendered = super
        return rendered if @diagnostic.nil?

        "#{rendered}\n#{@diagnostic}"
      end
    end
  end
end
