# frozen_string_literal: true

require_relative "codec/error"
require_relative "codec/utils"
require_relative "codec/handle_walk"
require_relative "codec/factory"
require_relative "codec/encoder"
require_relative "codec/decoder"

module Kobako
  # Host-side MessagePack codec for the kobako wire contract — the
  # byte-level layer ({docs/wire-codec.md}[link:../../docs/wire-codec.md]).
  # Two consumers sit on top:
  # +Kobako::Transport+ pins the host↔guest framing (Request / Response /
  # Run / Yield) and +Kobako::Outcome+ owns the per-+#run+ outcome
  # envelope (Result body / Panic map). The ext-type leaves this layer
  # carries — +Kobako::Handle+ (0x01) and +Kobako::Fault+ (0x02) — live at
  # the kobako root so the codec can register them without depending
  # upward on Transport.
  #
  # Backed by the official +msgpack+ gem via {Factory}; {Encoder} and
  # {Decoder} are thin wrappers that register the three kobako-specific
  # ext types (0x00 Symbol, 0x01 Capability Handle, 0x02 Exception
  # envelope) on a single +MessagePack::Factory+ instance. The Rust side
  # mirrors this layer as the +codec+ module in the +kobako-codec+ crate;
  # the ext-code constants live as module-private values on {Factory}
  # alongside +codec::EXT_SYMBOL+ / +codec::EXT_HANDLE+ /
  # +codec::EXT_ERRENV+ on that side.
  module Codec
  end
end
