# frozen_string_literal: true

require_relative "../errors"

module Kobako
  module Transport
    # The guest broke the exchange with the host: it produced a value that
    # could not be decoded, or a dispatch it made could not complete. Being a
    # SandboxError, it is caught by rescuing that too.
    class Error < Kobako::SandboxError
      def initialize(message, diagnostic: nil, **) # :nodoc:
        super(message, **)
        @diagnostic = diagnostic
      end

      # Appends the codec's own reason, which an operator needs and a caller
      # cannot act on, so #message leaves it out.
      def detailed_message(...)
        rendered = super
        return rendered if @diagnostic.nil?

        "#{rendered}\n#{@diagnostic}"
      end
    end
  end
end
