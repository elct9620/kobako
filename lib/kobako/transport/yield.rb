# frozen_string_literal: true

require_relative "../codec"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns the +Yield+ envelope value object plus its +#encode+ / +.decode+
  # codec for the +__kobako_yield_to_block+ wire form.
  module Transport
    # First byte of the YieldResponse for the success branch — body is
    # the block's return value encoded as a single msgpack value.
    TAG_OK = 0x01
    # First byte for +break val+ — body is the break value.
    TAG_BREAK = 0x02
    # Reserved for future +return val+ support; both sides reject this
    # tag as a wire violation (YieldResponse envelope contract).
    TAG_RESERVED = 0x03
    # First byte for an error / fault outcome — body is a
    # +{"class", "message", "backtrace"}+ Hash.
    TAG_ERROR = 0x04

    # Tags both sides currently accept on the wire.
    LIVE_TAGS = [TAG_OK, TAG_BREAK, TAG_ERROR].freeze

    # Value object for a single YieldResponse envelope
    # ({docs/wire-codec.md YieldResponse Envelope}[link:../../../docs/wire-codec.md]).
    #
    # The wire form is a one-byte tag followed by an msgpack payload.
    # The three live tags are +0x01+ (ok), +0x02+ (break), and +0x04+
    # (error); +0x03+ is reserved and rejected by both sides.
    #
    # +value+ carries whatever the wire payload decoded to — a plain
    # Ruby value for the +ok+ / +break+ tags, and a +{"class",
    # "message", "backtrace"}+ Hash for the +error+ tag. No further
    # shape constraint is enforced here; the host-side dispatcher
    # decides how to translate each variant into Ruby control flow.
    #
    # Lives alongside the other envelope value objects (+Request+,
    # +Response+) since it is the guest-to-host shape used
    # mid-dispatch-frame to answer a +__kobako_yield_to_block+ re-entry.
    class Yield < Data.define(:tag, :value)
      def initialize(tag:, value:)
        unless Kobako::Transport::LIVE_TAGS.include?(tag)
          raise ArgumentError,
                "Yield tag must be one of #{Kobako::Transport::LIVE_TAGS.inspect}, got #{tag.inspect}"
        end

        super
      end

      def ok?    = tag == Kobako::Transport::TAG_OK
      def break? = tag == Kobako::Transport::TAG_BREAK
      def error? = tag == Kobako::Transport::TAG_ERROR

      # Encode this Yield to YieldResponse bytes: one tag byte followed
      # by an msgpack-encoded +value+.
      def encode
        [tag].pack("C") + Codec::Encoder.encode(value)
      end

      # Decode +bytes+ into a Yield. Rejects empty input, the reserved
      # tag 0x03, and any tag outside +LIVE_TAGS+ by raising
      # +Kobako::Codec::InvalidType+ — these are wire violations per the
      # SPEC's YieldResponse envelope contract.
      def self.decode(bytes)
        bytes = bytes.b
        raise Codec::InvalidType, "YieldResponse must carry at least one byte" if bytes.empty?

        tag = bytes.getbyte(0) # : Integer
        body = bytes.byteslice(1, bytes.bytesize - 1) # : String

        reject_dead_tag!(tag)
        new(tag: tag, value: Codec::Decoder.decode(body))
      end

      def self.reject_dead_tag!(tag)
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
      private_class_method :reject_dead_tag!
    end
  end
end
