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
    # {Decoder} are thin wrappers that register the three kobako-specific
    # ext types (0x00 Symbol, 0x01 Capability Handle, 0x02 Exception
    # envelope) on a single +MessagePack::Factory+ instance. The Rust side
    # mirrors this layer as the +codec+ module in the +kobako-wasm+ crate;
    # the ext-code constants live as module-private values on {Factory}
    # alongside +codec::EXT_SYMBOL+ / +codec::EXT_HANDLE+ /
    # +codec::EXT_ERRENV+ on that side.
    module Codec
      # Wire-boundary translator: every wire Value Object (Handle /
      # Exception / Request / Response / Panic / Outcome) raises
      # +ArgumentError+ when an invariant is violated at construction.
      # The wire boundary surfaces those violations to callers as
      # +InvalidType+ so the public taxonomy stays +Codec::Error+ and
      # never leaks +ArgumentError+ from the Ruby standard library.
      #
      # Wrap any block that constructs a wire Value Object from decoded
      # bytes with this helper to keep the five decode sites (Request /
      # Response / Panic / Handle ext / Exception ext) uniform. Do not
      # use it for general-purpose validation outside the wire boundary
      # — host-layer +ArgumentError+ values should propagate unchanged.
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
