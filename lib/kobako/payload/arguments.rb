# frozen_string_literal: true

require_relative "../codec"

module Kobako
  module Payload
    class Arguments < Data.define(:args, :kwargs) # :nodoc:
      def initialize(args: [], kwargs: {})
        raise ArgumentError, "payload args must be Array" unless args.is_a?(Array)

        validate_kwargs!(kwargs)
        super
      end

      # Encode to the +[args, kwargs]+ msgpack bytes. The value object's
      # own invariants are the contract; this does not re-check the shape.
      def encode
        Codec::Encoder.encode([args, kwargs])
      end

      def self.decode(bytes)
        Codec::Decoder.decode(bytes) do |frame|
          unless frame.is_a?(Array) && frame.length == 2
            raise Codec::InvalidTypeError,
                  "an invocation payload is malformed (expected a 2-element array)"
          end

          args, kwargs = frame
          new(args: args, kwargs: kwargs)
        end
      end

      private

      def validate_kwargs!(kwargs)
        raise ArgumentError, "payload kwargs must be Hash" unless kwargs.is_a?(Hash)

        kwargs.each_key do |key|
          raise ArgumentError, "payload kwargs keys must be Symbol, got #{key.class}" unless key.is_a?(Symbol)
        end
      end
    end
  end
end
