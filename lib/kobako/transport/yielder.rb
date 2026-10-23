# frozen_string_literal: true

require_relative "../codec"
require_relative "../errors"

module Kobako
  module Transport
    class Yielder # :nodoc:
      def initialize(guest_yielder, break_tag, handles, failure)
        @guest_yielder = guest_yielder
        @break_tag = break_tag
        @handles = handles
        @failure = failure
        @active = true
      end

      # The ok value is consumed by the host Service, so a Capability Handle
      # in it is restored. A block exit stays in the guest, so the Service
      # is only unwound.
      def yield(*args)
        raise LocalJumpError, "guest block invoked after host dispatch frame returned" unless @active

        arm, body, klass = @guest_yielder.call(encode_args(args))
        raise UnsupportedError, "the Sandbox runtime does not support yielding to a block" if arm == :unsupported
        raise @failure.remember(BlockError.new(body, klass: klass)) if arm == :error

        throw @break_tag if arm == :break

        restore(*decode_body(body))
      end

      def to_proc
        method(:yield).to_proc
      end

      def invalidate
        @active = false
      end

      private

      # Restated so the Service reads a refusal of its own argument rather
      # than a codec class it never named.
      def encode_args(args)
        Kobako::Codec::Nesting.assert_within_bound!(args)
        Kobako::Codec::Encoder.encode(args)
      rescue Kobako::Codec::Error => e
        raise YieldValueError, "Service yielded a value the block cannot receive: #{e.message}"
      end

      # The tracking bracket opens only around this decode: the guest
      # re-entry may run nested dispatches whose own brackets would
      # otherwise pollute the signal.
      def decode_body(body)
        Kobako::Codec.track_handles { Kobako::Codec::Decoder.decode(body) }
      end

      def restore(value, carried_handle)
        return value unless carried_handle

        Kobako::Codec::HandleWalk.deep_restore(value, @handles)
      end
    end
  end
end
