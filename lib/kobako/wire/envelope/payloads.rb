# frozen_string_literal: true

require_relative "../codec"

module Kobako
  module Wire
    # Outcome-path envelopes (SPEC.md Outcome Envelope): Result and Panic
    # value objects plus the tagged Outcome wrapper that frames them on
    # the wire. The RPC-path counterparts (Request / Response) live in
    # the parent +envelope.rb+ file.
    module Envelope
      # ============================================================
      # Result envelope (SPEC.md Outcome Envelope → Result)
      # ============================================================
      #
      # Success Outcome payload. SPEC pins the Result envelope as the
      # raw msgpack encoding of the value, with no enclosing array —
      # the outer Outcome tag byte (0x01) is the sole discriminator.

      def self.encode_result(value)
        Codec::Encoder.encode(value)
      end

      def self.decode_result(bytes)
        Codec::Decoder.decode(bytes)
      end

      # ============================================================
      # Panic (SPEC.md Outcome Envelope → Panic)
      # ============================================================
      #
      # Failure Outcome payload. Encoded as a msgpack **map** keyed by
      # name (forward-compatibility — unknown keys are silently ignored).
      # Required: "origin" / "class" / "message". Optional: "backtrace"
      # (array of str), "details" (any wire-legal value).
      Panic = Data.define(:origin, :klass, :message, :backtrace, :details) do
        # steep:ignore:start
        def initialize(origin:, klass:, message:, backtrace: [], details: nil)
          raise ArgumentError, "Panic origin must be String"  unless origin.is_a?(String)
          raise ArgumentError, "Panic class must be String"   unless klass.is_a?(String)
          raise ArgumentError, "Panic message must be String" unless message.is_a?(String)
          unless backtrace.is_a?(Array) && backtrace.all?(String)
            raise ArgumentError, "Panic backtrace must be Array of String"
          end

          super
        end
        # steep:ignore:end
      end

      Panic::ORIGIN_SANDBOX = "sandbox"
      Panic::ORIGIN_SERVICE = "service"

      def self.encode_panic(panic)
        Codec::Encoder.encode(panic_map(panic))
      end

      # SPEC: Panic is a msgpack MAP keyed by name. Required keys always
      # emitted; "backtrace" emitted only when non-empty (keep the wire
      # compact); "details" only when non-nil. Ruby Hash preserves
      # insertion order so the resulting msgpack map carries the keys in
      # the order added below.
      def self.panic_map(panic)
        map = { "origin" => panic.origin, "class" => panic.klass, "message" => panic.message } # : Hash[String, untyped]
        map["backtrace"] = panic.backtrace unless panic.backtrace.empty?
        map["details"]   = panic.details   unless panic.details.nil?
        map
      end
      private_class_method :panic_map

      def self.decode_panic(bytes)
        map = Codec::Decoder.decode(bytes)
        raise Codec::InvalidType, "Panic envelope must be a map, got #{map.class}" unless map.is_a?(Hash)

        Codec.translate_value_object_error do
          Panic.new(
            origin: map["origin"], klass: map["class"], message: map["message"],
            backtrace: map["backtrace"] || [], details: map["details"]
          )
        end
      end

      # ============================================================
      # Outcome (SPEC.md Outcome Envelope)
      # ============================================================
      #
      # OUTCOME_BUFFER wrapper: one-byte tag (+0x01+ success-value, +0x02+
      # Panic) followed by the msgpack payload. The success payload is the
      # bare msgpack encoding of the returned value; the failure payload
      # is a Panic map. Construct +Outcome.new(value)+ for the success
      # branch or +Outcome.new(panic)+ for the failure branch.
      Outcome = Data.define(:payload) do
        # steep:ignore:start
        def result? = !payload.is_a?(Panic)
        def panic?  = payload.is_a?(Panic)
        # steep:ignore:end
      end

      def self.encode_outcome(outcome)
        tag, body = encode_outcome_payload(outcome.payload)
        out = String.new(encoding: Encoding::ASCII_8BIT)
        out << [tag].pack("C")
        out << body
        out
      end

      def self.encode_outcome_payload(payload)
        if payload.is_a?(Panic)
          [OUTCOME_TAG_PANIC, encode_panic(payload)]
        else
          [OUTCOME_TAG_RESULT, encode_result(payload)]
        end
      end
      private_class_method :encode_outcome_payload

      def self.decode_outcome(bytes)
        bytes = bytes.b
        raise Codec::InvalidType, "Outcome bytes must not be empty" if bytes.empty?

        tag = bytes.getbyte(0) # : Integer
        body = bytes.byteslice(1, bytes.bytesize - 1) # : String
        Outcome.new(decode_outcome_payload(tag, body))
      end

      def self.decode_outcome_payload(tag, body)
        case tag
        when OUTCOME_TAG_RESULT then decode_result(body)
        when OUTCOME_TAG_PANIC  then decode_panic(body)
        else raise Codec::InvalidType, format("unknown outcome tag 0x%<tag>02x", tag: tag)
        end
      end
      private_class_method :decode_outcome_payload
    end
  end
end
