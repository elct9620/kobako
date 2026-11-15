# frozen_string_literal: true

require_relative "../handle"
require_relative "../names"
require_relative "../codec"
require_relative "../payload"
require_relative "argument_wrap"

module Kobako
  module Transport
    # A Handle already in the arguments is refused, since a caller never
    # legitimately holds one.
    class Run < Data.define(:entrypoint, :args, :kwargs) # :nodoc:
      def initialize(entrypoint:, args: [], kwargs: {})
        entrypoint = Kobako.to_constant_name(entrypoint, "entrypoint")
        validate_args(args)
        validate_kwargs(kwargs)
        super
      end

      # A leaf with no wire form becomes a Capability Handle in the
      # invocation's table. The walk starts one level down, since +args+ and
      # +kwargs+ ride inside the payload document the wire's nesting bound
      # is counted from.
      def encode_payload(handles)
        Payload::Arguments.new(
          args: ArgumentWrap.wrap(args, handles, 1),
          kwargs: ArgumentWrap.wrap(kwargs, handles, 1)
        ).encode
      end

      private

      def validate_args(args)
        raise TypeError, "arguments must be an Array" unless args.is_a?(Array)
        raise ArgumentError, forged_handle_message("arguments") if args.any?(Kobako::Handle)
      end

      # Checked here so the Host App sees the host-side message before any
      # encode boundary.
      def validate_kwargs(kwargs)
        raise TypeError, "keyword arguments must be a Hash" unless kwargs.is_a?(Hash)

        bad_keys = kwargs.each_key.grep_v(Symbol)
        unless bad_keys.empty?
          raise TypeError,
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
