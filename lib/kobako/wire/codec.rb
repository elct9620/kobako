# frozen_string_literal: true

require_relative "codec/error"

module Kobako
  module Wire
    # Host-side MessagePack codec for the kobako wire contract — the
    # byte-level layer (SPEC.md → Wire Codec). The envelope layer
    # (Kobako::Wire::Envelope) sits on top of this and pins the four
    # logical message shapes (Request / Response / Result / Panic).
    #
    # Backed by the official +msgpack+ gem via {Factory}; {Encoder} and
    # {Decoder} are thin wrappers that register the two kobako-specific
    # ext types (0x01 Capability Handle and 0x02 Exception envelope) on
    # a single +MessagePack::Factory+ instance. The Rust side mirrors
    # this layer as the +codec+ module in the +kobako-wasm+ crate; the
    # ext-code constants live as module-private values on {Factory}
    # alongside +codec::EXT_HANDLE+ / +codec::EXT_ERRENV+ on that side.
    module Codec
      # Boundary translator: every Value Object in the wire layer
      # (Handle / Exception / Request / Response / Panic / ...) raises
      # +ArgumentError+ when an invariant is violated at construction.
      # The wire boundary surfaces those violations to callers as
      # +InvalidType+ so the public taxonomy is +Codec::Error+ subclasses
      # and never +ArgumentError+ from the Ruby standard library.
      #
      # Use this helper around any block that constructs a Value Object
      # from wire-decoded data so the translation is uniform across the
      # five decode sites (Request / Response / Panic / Handle ext /
      # Exception ext).
      def self.translate_value_object_error
        yield
      rescue ::ArgumentError => e
        raise InvalidType, e.message
      end
    end
  end
end

require_relative "codec/factory"
require_relative "codec/encoder"
require_relative "codec/decoder"
