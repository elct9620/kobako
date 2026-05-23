# frozen_string_literal: true

require_relative "../transport"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns the +Fault+ value object that backs the ext 0x02 Exception
  # envelope.
  module Transport
    # Wire-level value object for an ext-0x02 Exception envelope.
    #
    # SPEC pins the payload
    # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Ext Types
    # → ext 0x02) to a msgpack map with exactly three keys:
    #   * "type"    — one of "runtime", "argument", "disconnected", "undefined"
    #   * "message" — human-readable string
    #   * "details" — any wire-legal value, or nil when absent
    #
    # This object holds the *encoded* form. Reifying the corresponding Ruby
    # exception class (RuntimeError, ArgumentError, Kobako::ServiceError, ...)
    # is the responsibility of the dispatch layer, not the codec.
    #
    # Built on the +class X < Data.define(...)+ subclass form so the
    # class body is fully Steep-visible; ruby/rbs upstream documents
    # this as the Steep-friendly shape and the +Style/DataInheritance+
    # cop is disabled on that basis (see +.rubocop.yml+).
    class Fault < Data.define(:type, :message, :details)
      VALID_TYPES = %w[runtime argument disconnected undefined].freeze

      def initialize(type:, message:, details: nil)
        raise ArgumentError, "type must be String"    unless type.is_a?(String)
        raise ArgumentError, "message must be String" unless message.is_a?(String)
        raise ArgumentError, "type=#{type.inspect} not one of #{VALID_TYPES.inspect}" unless VALID_TYPES.include?(type)

        super
      end
    end
  end
end
