# frozen_string_literal: true

require_relative "../codec"
require_relative "../transport"
require_relative "yield"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns the host-side Proc factory that materialises guest-supplied
  # blocks as Ruby callables.
  module Transport
    # Host-side yield Proc factory for guest-supplied blocks (B-23).
    #
    # Each guest call that carries +block_given: true+ gets a Proc that
    # the Dispatcher hands to the Service method as +&block+. The Proc
    # serialises positional yield args, re-enters the guest via the
    # injected +yield_to_guest+ lambda
    # ({BRIDGE_REDESIGN §5.5.3}), and reifies the +YieldResponse+ into
    # Ruby control flow:
    #
    #   * +tag 0x01+ ok    — return the decoded value to +yield+'s caller
    #   * +tag 0x02+ break — +throw break_tag, value+ so the
    #     Dispatcher's +catch+ frame unwinds the Service method
    #     ({docs/behavior.md B-25}[link:../../../docs/behavior.md])
    #   * +tag 0x04+ error — raise the +{class, message}+ payload at the
    #     Service's yield site
    #
    # A paired invalidator lambda is returned alongside the proxy; the
    # Dispatcher's +ensure+ block calls it after dispatch completes so
    # any later call to a stashed proxy raises +LocalJumpError+ — the
    # observable shape of {docs/behavior.md E-23}[link:../../../docs/behavior.md]
    # (escaped yield proxy).
    module YieldProxy
      module_function

      # Build a +[proxy, invalidator]+ pair. +yield_to_guest+ is a
      # +String → String+ callable (typically +Runtime#yield_to_active_invocation+
      # bound through a lambda) that the proxy invokes to re-enter the
      # guest; +break_tag+ is the +catch+ throw tag the Dispatcher will
      # match against to unwind the Service on +tag 0x02+.
      def build(yield_to_guest, break_tag)
        frame_active = true
        invalidator = -> { frame_active = false }
        proxy = proc do |*args|
          raise LocalJumpError, "guest block invoked after host dispatch frame returned" unless frame_active

          response = Kobako::Transport.decode_yield(yield_to_guest.call(Kobako::Codec::Encoder.encode(args)))
          next response.value if response.ok?

          throw break_tag, response.value if response.break?

          raise yield_failure(response.value, default: "yield error")
        end
        [proxy, invalidator]
      end

      # Reify a +YieldResponse+ tag 0x04 payload into a +RuntimeError+
      # the Service method observes at its +yield+ site. The
      # +{class, message, backtrace}+ shape mirrors the
      # +Kobako::Transport::Yield+ tag 0x04 payload; +default+ provides
      # a fallback when the payload is not a Hash.
      def yield_failure(payload, default:)
        return RuntimeError.new(default) unless payload.is_a?(Hash)

        klass = payload["class"] || "RuntimeError"
        message = payload["message"] || default
        RuntimeError.new("#{klass}: #{message}")
      end
    end
  end
end
