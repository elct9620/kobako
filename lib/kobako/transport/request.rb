# frozen_string_literal: true

require_relative "../handle"
require_relative "../codec"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns the Request value object and its +#encode+ / +.decode+ codec.
  module Transport
    # Value object for a single guest-initiated Transport Request
    # ({docs/wire-codec.md Envelope Encoding → Request}[link:../../../docs/wire-codec.md]).
    #
    # 5-element msgpack array:
    # +[target, method_name, args, kwargs, block_given]+. +target+ is
    # either a +String+ (+"<Namespace>::<Member>"+, e.g. +"MyService::KV"+)
    # or a Handle. SPEC pins +kwargs+ map keys to ext 0x00 Symbol;
    # enforced at construction so the Value Object is the single source of
    # truth. +block_given+ is a Boolean signalling whether the guest call
    # site supplied a block; the block body itself never crosses the
    # wire.
    #
    # Built on the +class X < Data.define(...)+ subclass form so the
    # class body is fully Steep-visible; see +lib/kobako/outcome/panic.rb+
    # for the rationale.
    class Request < Data.define(:target, :method_name, :args, :kwargs, :block_given)
      def initialize(target:, method_name:, args: [], kwargs: {}, block_given: false)
        unless target.is_a?(String) || target.is_a?(Kobako::Handle)
          raise ArgumentError, "Request target must be String or Kobako::Handle, got #{target.class}"
        end
        raise ArgumentError, "Request method_name must be String" unless method_name.is_a?(String)
        raise ArgumentError, "Request args must be Array"         unless args.is_a?(Array)
        unless block_given.is_a?(TrueClass) || block_given.is_a?(FalseClass)
          raise ArgumentError, "Request block_given must be Boolean, got #{block_given.class}"
        end

        validate_kwargs!(kwargs)
        super
      end

      # Encode this Request to msgpack bytes. The Value Object's own
      # invariants are the contract; this method does not re-check the shape.
      def encode
        Codec::Encoder.encode([target, method_name, args, kwargs, block_given])
      end

      # Decode +bytes+ into a Request. Raises +Codec::InvalidType+ when the
      # envelope is not the expected 5-element msgpack array, or when the
      # Value Object's construction invariants reject the decoded fields.
      def self.decode(bytes)
        Codec::Decoder.decode(bytes) do |arr|
          unless arr.is_a?(Array) && arr.length == 5
            raise Codec::InvalidType, "Request envelope is malformed (expected a 5-element array)"
          end

          target, method_name, args, kwargs, block_given = arr
          new(target: target, method_name: method_name, args: args, kwargs: kwargs, block_given: block_given)
        end
      end

      private

      def validate_kwargs!(kwargs)
        raise ArgumentError, "Request kwargs must be Hash" unless kwargs.is_a?(Hash)

        kwargs.each_key do |k|
          raise ArgumentError, "Request kwargs keys must be Symbol, got #{k.class}" unless k.is_a?(Symbol)
        end
      end
    end
  end
end
