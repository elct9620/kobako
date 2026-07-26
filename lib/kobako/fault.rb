# frozen_string_literal: true

module Kobako
  # Wire-level value object for a Fault (ext 0x02).
  #
  # Top-level shared wire primitive: like +Kobako::Handle+ (ext 0x01),
  # +Fault+ is a MessagePack ext-type leaf registered by
  # +Kobako::Codec::ExtTypes+ and rides as the body of a Reply's fault
  # arm. It lives at the kobako root rather than under +Transport+
  # because the Codec layer must register it, and Codec must not depend
  # upward on Transport.
  #
  # SPEC pins the payload
  # ({docs/wire/payload-msgpack.md}[link:../../docs/wire/payload-msgpack.md] § Ext Types
  # → ext 0x02) to a msgpack map with exactly two keys:
  #   * "type"    — one of "runtime", "argument", "undefined"
  #   * "message" — human-readable string
  #
  # A Fault travels host→guest, so it carries only what its author can
  # keep bounded: the message a Service chose to expose. Host-side
  # structure — backtraces, paths, object graphs — never crosses.
  #
  # This object holds the *encoded* form. Reifying the corresponding Ruby
  # exception class (RuntimeError, ArgumentError, Kobako::ServiceError, ...)
  # is the responsibility of the dispatch layer, not the codec.
  #
  # Built on the +class X < Data.define(...)+ subclass form (the
  # Steep-friendly shape — see +.rubocop.yml+ for the rationale).
  class Fault < Data.define(:type, :message)
    VALID_TYPES = %w[runtime argument undefined].freeze

    def initialize(type:, message:)
      raise ArgumentError, "type must be String"    unless type.is_a?(String)
      raise ArgumentError, "message must be String" unless message.is_a?(String)
      raise ArgumentError, "type=#{type.inspect} not one of #{VALID_TYPES.inspect}" unless VALID_TYPES.include?(type)

      super
    end
  end
end
