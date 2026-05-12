# frozen_string_literal: true

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
    # this layer as the +codec+ module in the +kobako-wasm+ crate, and
    # the {EXT_HANDLE} / {EXT_ERRENV} constants below match
    # +codec::EXT_HANDLE+ / +codec::EXT_ERRENV+ in that crate.
    module Codec
      # MessagePack ext type code reserved for Capability Handle
      # (SPEC.md → Wire Codec → Ext Types → ext 0x01).
      EXT_HANDLE = 0x01

      # MessagePack ext type code reserved for Exception envelope
      # (SPEC.md → Wire Codec → Ext Types → ext 0x02).
      EXT_ERRENV = 0x02
    end
  end
end

require_relative "codec/error"
require_relative "codec/factory"
require_relative "codec/encoder"
require_relative "codec/decoder"
