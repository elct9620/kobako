# frozen_string_literal: true

require_relative "codec"
require_relative "yield/response"

module Kobako
  # Host-facing boundary for the YieldResponse envelope produced by
  # +__kobako_yield_to_block+ ({docs/wire-codec.md YieldResponse
  # Envelope}[link:../../docs/wire-codec.md]). Takes raw response bytes
  # — a one-byte tag followed by an msgpack payload — and maps them to a
  # {Response} value object the dispatcher can branch on.
  #
  # The wire format mirrors {Kobako::Outcome} (one tag byte plus a
  # single msgpack value) but with a different tag set. Lives here as a
  # peer of +RPC+ and +Outcome+ — the {Kobako::RPC::Envelope::Response}
  # is host-to-guest after a Request, +Kobako::Outcome+ is guest-to-host
  # at end-of-invocation, and {Kobako::Yield} is guest-to-host mid-
  # dispatch-frame.
  #
  #   * tag 0x01 ok         → Response.ok(value)
  #   * tag 0x02 break      → Response.break_(value)
  #   * tag 0x03 (reserved) → +Kobako::Codec::InvalidType+
  #   * tag 0x04 error      → Response.error(payload)
  #   * empty / unknown tag → +Kobako::Codec::InvalidType+
  module Yield
    # First byte of the YieldResponse for the success branch — body is
    # the block's return value encoded as a single msgpack value.
    TAG_OK = 0x01
    # First byte for `break val` — body is the break value.
    TAG_BREAK = 0x02
    # Reserved for future `return val` support; both sides currently
    # reject this tag as a wire violation (BLOCK_RESEARCH (d)).
    TAG_RESERVED = 0x03
    # First byte for an error / fault outcome — body is a
    # +{"class", "message", "backtrace"}+ Hash.
    TAG_ERROR = 0x04

    # Tags both sides currently accept on the wire.
    LIVE_TAGS = [TAG_OK, TAG_BREAK, TAG_ERROR].freeze

    module_function

    # Encode +response+ to YieldResponse bytes: one tag byte followed
    # by an msgpack-encoded +value+.
    def encode_response(response)
      [response.tag].pack("C") + Codec::Encoder.encode(response.value)
    end

    # Decode +bytes+ into a {Response}. Rejects empty input, the
    # reserved tag 0x03, and any tag outside +LIVE_TAGS+ by raising
    # +Kobako::Codec::InvalidType+ — these are wire violations per the
    # SPEC's YieldResponse envelope contract.
    def decode_response(bytes)
      bytes = bytes.b
      raise Codec::InvalidType, "YieldResponse must carry at least one byte" if bytes.empty?

      tag = bytes.getbyte(0) # : Integer
      body = bytes.byteslice(1, bytes.bytesize - 1) || +""

      reject_dead_tag!(tag)
      Response.new(tag: tag, value: Codec::Decoder.decode(body))
    end

    def reject_dead_tag!(tag)
      return if LIVE_TAGS.include?(tag)

      msg = if tag == TAG_RESERVED
              "YieldResponse tag 0x03 is reserved"
            else
              format(
                "YieldResponse tag 0x%02x is not recognised", tag
              )
            end
      raise Codec::InvalidType, msg
    end
  end
end
