# frozen_string_literal: true

require_relative "../exception"
require_relative "../codec"

module Kobako
  module Wire
    # Envelope-layer value objects and encode/decode helpers. See envelope.rb.
    module Envelope
      # Response envelope (SPEC.md Wire Codec → Response).
      #
      # 2-element msgpack array: [status, value-or-error]. +status+ is 0
      # (success) or 1 (error). For success the second element is the
      # return value; for error it is an {Exception} (ext 0x02 envelope).
      #
      # The two factory methods (+ok+, +err+) reflect the two mutually
      # exclusive variants pinned by SPEC. Frozen value object backed by
      # +Data.define+. Equality, +eql?+, and +hash+ are provided
      # automatically based on field values.
      Response = Data.define(:status, :payload) do
        def self.ok(value)
          new(status: STATUS_OK, payload: value)
        end

        def self.err(exception)
          unless exception.is_a?(Exception)
            raise ArgumentError, "Response.err requires Kobako::Wire::Exception, got #{exception.class}"
          end

          new(status: STATUS_ERROR, payload: exception)
        end

        def initialize(status:, payload:)
          unless [STATUS_OK, STATUS_ERROR].include?(status)
            raise ArgumentError, "Response status must be 0 or 1, got #{status.inspect}"
          end
          if status == STATUS_ERROR && !payload.is_a?(Exception)
            raise ArgumentError, "Response status=1 payload must be Kobako::Wire::Exception"
          end

          super
        end

        def ok?
          status == STATUS_OK
        end

        def err?
          status == STATUS_ERROR
        end
      end

      # ---------------- Response encode / decode ----------------

      def self.encode_response(response)
        raise ArgumentError, "encode_response requires Response" unless response.is_a?(Response)

        Encoder.encode([response.status, response.payload])
      end

      def self.decode_response(bytes)
        arr = Decoder.decode(bytes)
        unless arr.is_a?(Array) && arr.length == 2
          raise InvalidType, "Response must be a 2-element array, got #{arr.inspect}"
        end

        decode_response_status(*arr)
      end

      def self.decode_response_status(status, payload)
        case status
        when STATUS_OK
          Response.new(status: STATUS_OK, payload: payload)
        when STATUS_ERROR
          raise InvalidType, "Response status=1 payload must be ext 0x02 Exception" unless payload.is_a?(Exception)

          Response.new(status: STATUS_ERROR, payload: payload)
        else
          raise InvalidType, "Response status must be 0 or 1, got #{status.inspect}"
        end
      end
      private_class_method :decode_response_status
    end
  end
end
