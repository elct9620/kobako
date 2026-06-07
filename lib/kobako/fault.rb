# frozen_string_literal: true

module Kobako
  # Wire-level value object for an ext-0x02 Exception envelope.
  #
  # Top-level shared wire primitive: like +Kobako::Handle+ (ext 0x01),
  # +Fault+ is a MessagePack ext-type leaf registered by
  # +Kobako::Codec::Factory+ and rides nested inside other envelopes (a
  # +Kobako::Transport::Response+ error payload, or another Fault's
  # +details+). It lives at the kobako root rather than under +Transport+
  # because the Codec layer must register it, and Codec must not depend
  # upward on Transport.
  #
  # SPEC pins the payload
  # ({docs/wire-codec.md}[link:../../docs/wire-codec.md] § Ext Types
  # → ext 0x02) to a msgpack map with exactly three keys:
  #   * "type"    — one of "runtime", "argument", "undefined"
  #   * "message" — human-readable string
  #   * "details" — any wire-legal value, or nil when absent
  #
  # This object holds the *encoded* form. Reifying the corresponding Ruby
  # exception class (RuntimeError, ArgumentError, Kobako::ServiceError, ...)
  # is the responsibility of the dispatch layer, not the codec.
  #
  # Built on the +class X < Data.define(...)+ subclass form (the
  # Steep-friendly shape — see +lib/kobako/outcome/panic.rb+).
  class Fault < Data.define(:type, :message, :details)
    VALID_TYPES = %w[runtime argument undefined].freeze

    def initialize(type:, message:, details: nil)
      raise ArgumentError, "type must be String"    unless type.is_a?(String)
      raise ArgumentError, "message must be String" unless message.is_a?(String)
      raise ArgumentError, "type=#{type.inspect} not one of #{VALID_TYPES.inspect}" unless VALID_TYPES.include?(type)

      super
    end
  end
end
