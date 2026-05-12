# frozen_string_literal: true

require_relative "../codec"

module Kobako
  module Wire
    # Envelope-layer value objects and encode/decode helpers. See envelope.rb.
    module Envelope
      # Result envelope (SPEC.md Outcome Envelope → Result).
      #
      # The successful Outcome payload. Wraps the deserialized last
      # expression of the mruby script. SPEC pins the Result envelope as
      # a 1-element msgpack array carrying the value, so that the framing
      # is symmetric with the Panic envelope and the value position is
      # never ambiguous.
      Result = Data.define(:value)

      # ---------------- Result encode / decode ----------------

      def self.encode_result(value)
        Encoder.encode([value])
      end

      def self.decode_result(bytes)
        arr = Decoder.decode(bytes)
        unless arr.is_a?(Array) && arr.length == 1
          raise InvalidType, "Result envelope must be a 1-element array, got #{arr.inspect}"
        end

        Result.new(arr[0])
      end
    end
  end
end
