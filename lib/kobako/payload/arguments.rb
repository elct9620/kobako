# frozen_string_literal: true

require_relative "../codec"

module Kobako
  module Payload
    # The invocation arguments a Call or a Run carries: a 2-element
    # msgpack array, +args+ then +kwargs+. Both elements are always
    # present, so field positions stay stable when either is empty.
    #
    # The positional-versus-keyword split lives here rather than in the
    # core envelope because it is Ruby's call semantics, not the wire's.
    # SPEC pins +kwargs+ keys to Symbols; the invariant is enforced at
    # construction so the value object is the single source of truth.
    #
    # Built on the +class X < Data.define(...)+ subclass form so the class
    # body is fully Steep-visible; see +lib/kobako/outcome/panic.rb+ for
    # the rationale.
    class Arguments < Data.define(:args, :kwargs)
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

      # Decode +bytes+ into an Arguments. Raises +Codec::InvalidType+ when
      # the payload is not the expected 2-element msgpack array, when
      # either position carries an ext 0x02 Fault (a Fault's only home is
      # a Reply's fault arm), or when the construction invariants reject
      # the decoded fields.
      def self.decode(bytes)
        Codec.forbid_faults do
          Codec::Decoder.decode(bytes) do |frame|
            unless frame.is_a?(Array) && frame.length == 2
              raise Codec::InvalidType,
                    "an invocation payload is malformed (expected a 2-element array)"
            end

            args, kwargs = frame
            new(args: args, kwargs: kwargs)
          end
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
