# frozen_string_literal: true

require_relative "result"
require_relative "panic"
require_relative "../encoder"
require_relative "../decoder"
require_relative "../error"

module Kobako
  module Wire
    # Envelope-layer value objects and encode/decode helpers. See envelope.rb.
    module Envelope
      # Outcome envelope (SPEC.md Outcome Envelope).
      #
      # The OUTCOME_BUFFER wrapper: a one-byte tag (+0x01+ result, +0x02+
      # panic) followed by the msgpack payload. Carries either a {Result}
      # or a {Panic}.
      #
      # Frozen value object backed by +Data.define+. Equality, +eql?+, and
      # +hash+ are provided automatically based on field values. Positional
      # construction (+Outcome.new(payload)+) is preserved for internal
      # callers; Data.new accepts both positional and keyword arguments.
      Outcome = Data.define(:payload) do
        def self.result(value)
          new(Result.new(value))
        end

        def self.panic(panic)
          raise ArgumentError, "Outcome.panic requires Panic" unless panic.is_a?(Panic)

          new(panic)
        end

        def initialize(payload:)
          unless payload.is_a?(Result) || payload.is_a?(Panic)
            raise ArgumentError, "Outcome payload must be Result or Panic, got #{payload.class}"
          end

          super
        end

        def result?
          payload.is_a?(Result)
        end

        def panic?
          payload.is_a?(Panic)
        end
      end

      # ---------------- Outcome encode / decode ----------------

      def self.encode_outcome(outcome)
        raise ArgumentError, "encode_outcome requires Outcome" unless outcome.is_a?(Outcome)

        tag, body = encode_outcome_payload(outcome.payload)
        out = String.new(encoding: Encoding::ASCII_8BIT)
        out << [tag].pack("C")
        out << body
        out
      end

      def self.encode_outcome_payload(payload)
        case payload
        when Result then [OUTCOME_TAG_RESULT, encode_result(payload.value)]
        when Panic  then [OUTCOME_TAG_PANIC, encode_panic(payload)]
        end
      end
      private_class_method :encode_outcome_payload

      def self.decode_outcome(bytes)
        bytes = bytes.b
        raise InvalidType, "Outcome bytes must not be empty" if bytes.empty?

        tag = bytes.getbyte(0)
        body = bytes.byteslice(1, bytes.bytesize - 1)
        Outcome.new(decode_outcome_payload(tag, body))
      end

      def self.decode_outcome_payload(tag, body)
        case tag
        when OUTCOME_TAG_RESULT then decode_result(body)
        when OUTCOME_TAG_PANIC  then decode_panic(body)
        else raise InvalidType, format("unknown outcome tag 0x%<tag>02x", tag: tag)
        end
      end
      private_class_method :decode_outcome_payload
    end
  end
end
