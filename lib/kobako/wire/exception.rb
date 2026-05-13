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
    #
    # Built on +Data.define+ so equality, hash, and immutability are
    # inherited from the value-object machinery; only the field invariants
    # ride on top.
    Exception = Data.define(:type, :message, :details) do
      def initialize(type:, message:, details: nil)
        valid_types = self.class::VALID_TYPES
        raise ArgumentError, "type must be String"    unless type.is_a?(String)
        raise ArgumentError, "message must be String" unless message.is_a?(String)
        raise ArgumentError, "type=#{type.inspect} not one of #{valid_types.inspect}" unless valid_types.include?(type)

        super
      end
    end

    Exception::VALID_TYPES = %w[runtime argument disconnected undefined].freeze
  end
end
