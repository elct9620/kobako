# frozen_string_literal: true

require_relative "../codec"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns the host-side object that materialises a guest-supplied block as
  # a Ruby callable the Service method can yield into.
  module Transport
    # Host-side stand-in for a guest-supplied block.
    #
    # Each guest call that carries +block_given: true+ gets a Yielder
    # that the Dispatcher hands to the Service method as +&block+. The
    # Service method observes it as an ordinary Ruby Proc through
    # #to_proc; +yield val+ / +block.call(val)+ invokes #yield, which
    # serialises the positional args, re-enters the guest via the injected
    # +yield_to_guest+ lambda, and reifies the Yield Reply into Ruby
    # control flow:
    #
    #   * ok    — return the decoded value to +yield+'s caller
    #   * break — +throw break_tag, value+ so the Dispatcher's +catch+
    #     frame unwinds the Service method
    #   * error — raise the guest's class and message at the Service's
    #     yield site
    #
    # The Dispatcher calls #invalidate! from its +ensure+ block once
    # dispatch completes; any later call to a stashed Yielder then raises
    # +LocalJumpError+ — the observable shape of an escaped Yielder.
    class Yielder
      # +yield_to_guest+ is the ext's per-dispatch
      # +Kobako::Runtime::GuestYielder+, which #yield invokes to re-enter
      # the guest: it takes the argument payload and answers the reply
      # already split into +[arm, body, class]+. +break_tag+ is the
      # +catch+ throw tag the Dispatcher matches against to unwind the
      # Service on a break. +handler+ is the invocation's +Kobako::Catalog::Handles+,
      # used to restore a Capability Handle in the block's ok value back to
      # its host object before it reaches the Service +yield+ site.
      def initialize(yield_to_guest, break_tag, handler)
        @yield_to_guest = yield_to_guest
        @break_tag = break_tag
        @handler = handler
        @active = true
      end

      # Re-enter the guest with +args+ and reify the Yield Reply into
      # Ruby control flow. Raises +LocalJumpError+ if called after
      # #invalidate!. The ok value is consumed by the host Service
      # method, so a Capability Handle in it is restored to its host object.
      # The break value unwinds past the Service back to the guest
      # bound-constant call, so it passes through verbatim — a Handle stays a
      # Handle and rides back on the same id rather than churning a new one.
      def yield(*args)
        raise LocalJumpError, "guest block invoked after host dispatch frame returned" unless @active

        arm, body, klass = @yield_to_guest.call(Kobako::Codec::Encoder.encode(args))
        raise "#{klass}: #{body}" if arm == :error

        value, carried_handle = decode_body(body)
        throw @break_tag, value if arm == :break

        restore(value, carried_handle)
      end

      # The Proc the Dispatcher passes as +&block+, binding #yield so a
      # Service method's +yield+ / +block.call+ drives the round-trip.
      def to_proc
        method(:yield).to_proc
      end

      # Mark this Yielder dead. Called by the Dispatcher's +ensure+ block
      # when the originating dispatch frame returns; any later #yield
      # call then raises +LocalJumpError+.
      def invalidate!
        @active = false
      end

      private

      # Decode a value-carrying arm's payload, answering the value and
      # whether the decode carried a Capability Handle. The tracking
      # bracket opens only around this decode: the guest re-entry may run
      # nested dispatches whose own brackets would otherwise pollute the
      # signal.
      def decode_body(body)
        Kobako::Codec.track_handles { Kobako::Codec::Decoder.decode(body) }
      end

      # Restore any Capability Handle in a block's ok value to its host
      # object via the injected +Catalog::Handles+. Only the
      # ok path calls this — host code consumes the ok value, whereas a
      # break value returns to the guest and stays a Handle. A response
      # whose decode carried no Handle resolves to itself, so the walk is
      # skipped entirely.
      def restore(value, carried_handle)
        return value unless carried_handle

        Kobako::Codec::HandleWalk.deep_restore(value, @handler)
      end
    end
  end
end
