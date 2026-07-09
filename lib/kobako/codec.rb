# frozen_string_literal: true

require_relative "codec/error"
require_relative "codec/utils"
require_relative "codec/handle_walk"
require_relative "codec/state"
require_relative "codec/ext_types"
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
  # Backed by the official +msgpack+ gem: ExtTypes registers the three
  # kobako-specific ext types (0x00 Symbol, 0x01 Capability Handle,
  # 0x02 Exception envelope) on one process-wide +MessagePack::Factory+,
  # and Encoder / Decoder are thin wrappers over it. The Rust side
  # mirrors this layer as the +codec+ module in the +kobako-codec+ crate;
  # the ext-code constants live as module-private values on ExtTypes
  # alongside +codec::EXT_SYMBOL+ / +codec::EXT_HANDLE+ /
  # +codec::EXT_ERRENV+ on that side.
  module Codec
    # Bracket a decode and return the block's result together with whether
    # the decoded tree carried an ext 0x01 Capability Handle — the signal a
    # dispatch path uses to skip an all-identity Handle-resolution walk.
    # The tracking state is codec-internal; this is its only readout.
    def self.track_handles(&block)
      State.current.track_handles(&block)
    end

    # Bracket a codec operation in a payload position: an ext 0x02 Fault
    # envelope is only legal in the Response fault field, so the envelope
    # layers open this bracket around every other encode / decode and the
    # ext-type conversions refuse the envelope while it is open — a wire
    # violation on decode, no wire representation on encode.
    def self.forbid_faults(&block)
      State.current.forbid_faults(&block)
    end
  end
end
