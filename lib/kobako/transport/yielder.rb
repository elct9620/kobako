# frozen_string_literal: true

require_relative "../codec"
require_relative "../errors"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns the host-side object that materialises a guest-supplied block as
  # a Ruby callable the Service method can yield into.
  module Transport
    class Yielder # :nodoc:
      def initialize(yield_to_guest, break_tag, handles)
        @yield_to_guest = yield_to_guest
        @break_tag = break_tag
        @handles = handles
        @active = true
        @raised = nil
      end

      # Identity rather than class: a Service that rescued the block's
      # failure and raised its own has reported something else, and the
      # guest must hear about that instead.
      #
      # The class travels with the message because not every block failure
      # is an exception the guest holds — a block value the guest refused
      # has a class to raise under and no object to continue.
      def fault_text(error)
        raised = @raised
        return if raised.nil? || !error.equal?(raised)

        "#{raised.klass}: #{raised.message}"
      end

      # The ok value is consumed by the host Service, so a Capability Handle
      # in it is restored. The break value unwinds back to the guest, so it
      # passes through verbatim and rides back on the same id rather than
      # churning a new one.
      def yield(*args)
        raise LocalJumpError, "guest block invoked after host dispatch frame returned" unless @active

        arm, body, klass = @yield_to_guest.call(encode_args(args))
        raise remember(BlockError.new(body, klass: klass)) if arm == :error

        value, carried_handle = decode_body(body)
        throw @break_tag, value if arm == :break

        restore(value, carried_handle)
      end

      def to_proc
        method(:yield).to_proc
      end

      def invalidate!
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

      # Only the newest is kept: a Service that rescued an earlier one and
      # yielded again has already handled it.
      def remember(error)
        @raised = error
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
