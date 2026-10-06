# frozen_string_literal: true

require_relative "../handle"
require_relative "../codec"
require_relative "../payload"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns +Run+, the host-side value object for one +#run+ request — the
  # native side frames it into the envelope +__kobako_run+ consumes.
  module Transport
    # A Handle already in the arguments is refused, since a caller never
    # legitimately holds one.
    class Run < Data.define(:entrypoint, :args, :kwargs) # :nodoc:
      # Ruby constant-name pattern enforced on the +entrypoint+ Symbol.
      # Parallel to
      # +Kobako::Catalog::Snippets::NAME_PATTERN+; the two constants name the
      # same regex but cover distinct surfaces (snippet identity vs.
      # entrypoint resolution) so a future divergence stays local.
      NAME_PATTERN = /\A[A-Z]\w*\z/

      def initialize(entrypoint:, args: [], kwargs: {})
        entrypoint = normalize_entrypoint(entrypoint)
        validate_args!(args)
        validate_kwargs!(kwargs)
        super
      end

      # A leaf with no wire form becomes a Capability Handle in the
      # invocation's table. The walk starts one level down, since +args+ and
      # +kwargs+ ride inside the payload document the wire's nesting bound
      # is counted from.
      def payload(handler)
        Payload::Arguments.new(
          args: Codec::HandleWalk.deep_wrap(args, handler, 1),
          kwargs: Codec::HandleWalk.deep_wrap(kwargs, handler, 1)
        ).encode
      end

      private

      # A target of the wrong type raises TypeError, a Host App programming
      # error caught before the run reaches the guest.
      def normalize_entrypoint(target)
        unless target.is_a?(Symbol) || target.is_a?(String)
          raise TypeError, "entrypoint must be a Symbol or String, got #{target.class}"
        end

        target_str = target.to_s
        unless NAME_PATTERN.match?(target_str)
          raise ArgumentError,
                "entrypoint must match #{NAME_PATTERN.inspect} (got #{target.inspect})"
        end

        target_str.to_sym
      end

      def validate_args!(args)
        raise ArgumentError, "arguments must be an Array" unless args.is_a?(Array)
        raise ArgumentError, forged_handle_message("arguments") if args.any?(Kobako::Handle)
      end

      # Checked here so the Host App sees the host-side message before any
      # encode boundary.
      def validate_kwargs!(kwargs)
        raise ArgumentError, "keyword arguments must be a Hash" unless kwargs.is_a?(Hash)

        bad_keys = kwargs.each_key.grep_v(Symbol)
        unless bad_keys.empty?
          raise ArgumentError,
                "keyword argument keys must be Symbols (got #{bad_keys.inspect})"
        end
        raise ArgumentError, forged_handle_message("keyword argument values") if kwargs.each_value.any?(Kobako::Handle)
      end

      def forged_handle_message(slot)
        "#{slot} must not contain a Kobako::Handle — " \
          "Handles are created internally by the Sandbox and cannot be passed in"
      end
    end
  end
end
