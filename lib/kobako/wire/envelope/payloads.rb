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
      # Result (SPEC.md Outcome Envelope → Result)
      # ============================================================
      #
      # Success Outcome payload. SPEC pins the Result envelope as a
      # 1-element msgpack array carrying the value, keeping framing
      # symmetric with the Panic envelope so the value position is never
      # ambiguous.
      Result = Data.define(:value)

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

      # ============================================================
      # Panic (SPEC.md Outcome Envelope → Panic)
      # ============================================================
      #
      # Failure Outcome payload. Encoded as a msgpack **map** keyed by
      # name (forward-compatibility — unknown keys are silently ignored).
      # Required: "origin" / "class" / "message". Optional: "backtrace"
      # (array of str), "details" (any wire-legal value).
      Panic = Data.define(:origin, :klass, :message, :backtrace, :details) do
        def initialize(origin:, klass:, message:, backtrace: [], details: nil)
          raise ArgumentError, "Panic origin must be String"  unless origin.is_a?(String)
          raise ArgumentError, "Panic class must be String"   unless klass.is_a?(String)
          raise ArgumentError, "Panic message must be String" unless message.is_a?(String)
          unless backtrace.is_a?(Array) && backtrace.all?(String)
            raise ArgumentError, "Panic backtrace must be Array of String"
          end

          super
        end
      end

      Panic::ORIGIN_SANDBOX = "sandbox"
      Panic::ORIGIN_SERVICE = "service"

      def self.encode_panic(panic)
        Encoder.encode(panic_map(panic))
      end

      # SPEC: Panic is a msgpack MAP keyed by name. Required keys always
      # emitted; "backtrace" emitted only when non-empty (keep the wire
      # compact); "details" only when non-nil. Ruby Hash preserves
      # insertion order so the resulting msgpack map carries the keys in
      # the order added below.
      def self.panic_map(panic)
        map = { "origin" => panic.origin, "class" => panic.klass, "message" => panic.message }
        map["backtrace"] = panic.backtrace unless panic.backtrace.empty?
        map["details"]   = panic.details   unless panic.details.nil?
        map
      end
      private_class_method :panic_map

      def self.decode_panic(bytes)
        map = Decoder.decode(bytes)
        raise InvalidType, "Panic envelope must be a map, got #{map.class}" unless map.is_a?(Hash)

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
      # OUTCOME_BUFFER wrapper: one-byte tag (+0x01+ Result, +0x02+ Panic)
      # followed by the msgpack payload of the corresponding envelope.
      # Callers construct an +Outcome+ by wrapping the payload directly —
      # +Outcome.new(Result.new(value))+ or +Outcome.new(panic)+ — so the
      # contract reads symmetrically across both variants.
      Outcome = Data.define(:payload) do
        def initialize(payload:)
          unless payload.is_a?(Result) || payload.is_a?(Panic)
            raise ArgumentError, "Outcome payload must be Result or Panic, got #{payload.class}"
          end

          super
        end

        def result? = payload.is_a?(Result)
        def panic?  = payload.is_a?(Panic)
      end

      def self.encode_outcome(outcome)
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
