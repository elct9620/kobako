# frozen_string_literal: true

require_relative "codec/error"

module Kobako
  # Host-side MessagePack codec for the kobako wire contract — the
  # byte-level layer (SPEC.md → Wire Codec). Two consumers sit on top:
  # +Kobako::RPC+ pins the RPC framing (Request / Response)
  # and +Kobako::Outcome+ owns the per-+#run+ outcome envelope (Result
  # body / Panic map).
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
  end
end

require_relative "codec/utils"
require_relative "codec/factory"
require_relative "codec/encoder"
require_relative "codec/decoder"
