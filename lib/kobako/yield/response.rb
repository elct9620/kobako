# frozen_string_literal: true

module Kobako
  module Yield
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
    # shape constraint is enforced here; callers in
    # +Kobako::RPC::Dispatcher+ (S5b+) decide how to translate each
    # variant into Ruby control flow.
    class Response < Data.define(:tag, :value)
      def initialize(tag:, value:)
        unless Kobako::Yield::LIVE_TAGS.include?(tag)
          raise ArgumentError,
                "Yield::Response tag must be one of #{Kobako::Yield::LIVE_TAGS.inspect}, got #{tag.inspect}"
        end

        super
      end

      def ok?    = tag == Kobako::Yield::TAG_OK
      def break? = tag == Kobako::Yield::TAG_BREAK
      def error? = tag == Kobako::Yield::TAG_ERROR
    end
  end
end
