# frozen_string_literal: true

module Kobako
  module Transport
    # The newest failure a guest block raised to a Service during one
    # invocation, whichever open Call carried that block: a Service may run
    # a block stored from an earlier Call, and its failure is still the
    # guest's own.
    class BlockFailure # :nodoc:
      def initialize
        @raised = nil
      end

      # Only the newest is kept: a Service that rescued an earlier one and
      # yielded again has already handled it.
      def remember(error)
        @raised = error
      end

      # Identity rather than class: a Service that rescued the block's
      # failure and raised its own has reported something else, and the
      # guest must hear about that instead.
      #
      # The class travels with the message because not every block failure
      # is an exception the guest holds — a block value the guest refused
      # has a class to raise under and no object to continue.
      def fault_text(error)
        raised = @raised
        return if raised.nil? || !error.equal?(raised)

        "#{raised.klass}: #{raised.message}"
      end
    end
  end
end
