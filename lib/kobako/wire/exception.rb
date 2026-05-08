# frozen_string_literal: true

module Kobako
  module Wire
    # Wire-level value object for an ext-0x02 Exception envelope.
    #
    # SPEC pins the payload (Wire Codec → Ext Types → ext 0x02) to a
    # msgpack map with exactly three keys:
    #   * "type"    — one of "runtime", "argument", "disconnected", "undefined"
    #   * "message" — human-readable string
    #   * "details" — any wire-legal value, or nil when absent
    #
    # This object holds the *encoded* form. Reifying the corresponding Ruby
    # exception class (RuntimeError, ArgumentError, Kobako::ServiceError, ...)
    # is the responsibility of the dispatch layer, not the codec.
    class Exception
      VALID_TYPES = %w[runtime argument disconnected undefined].freeze

      attr_reader :type, :message, :details

      def initialize(type:, message:, details: nil)
        raise ArgumentError, "type must be String"    unless type.is_a?(String)
        raise ArgumentError, "message must be String" unless message.is_a?(String)
        raise ArgumentError, "type=#{type.inspect} not one of #{VALID_TYPES.inspect}" unless VALID_TYPES.include?(type)

        @type = type
        @message = message
        @details = details
      end

      def ==(other)
        other.is_a?(Exception) &&
          other.type == @type &&
          other.message == @message &&
          other.details == @details
      end
      alias eql? ==

      def hash
        [self.class, @type, @message, @details].hash
      end

      def inspect
        "#<Kobako::Wire::Exception type=#{@type.inspect} message=#{@message.inspect} details=#{@details.inspect}>"
      end
    end
  end
end
