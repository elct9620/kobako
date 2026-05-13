# frozen_string_literal: true

require_relative "../codec"

module Kobako
  module Wire
    # Envelope-layer value objects and encode/decode helpers. See envelope.rb.
    module Envelope
      # Panic envelope (SPEC.md Outcome Envelope → Panic).
      #
      # The failure Outcome payload. Encoded as a msgpack **map** keyed
      # by name (forward-compatibility — unknown keys are silently
      # ignored). Required keys: "origin", "class", "message". Optional:
      # "backtrace" (array of str), "details" (any wire-legal value).
      #
      # Frozen value object backed by +Data.define+. Equality, +eql?+, and
      # +hash+ are provided automatically based on field values.
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

      # ---------------- Panic encode / decode ----------------

      def self.encode_panic(panic)
        raise ArgumentError, "encode_panic requires Panic" unless panic.is_a?(Panic)

        Encoder.encode(panic_map(panic))
      end

      # SPEC: Panic is a msgpack MAP keyed by name. We always emit the
      # required keys; "backtrace" is emitted only when non-empty (keep
      # the wire compact); "details" only when non-nil. Receivers must
      # ignore unknown keys, so the optional-key absence is wire-legal.
      # Ruby Hash preserves insertion order, so the resulting msgpack map
      # carries the keys in the order we add them.
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

        panic_from_map(map)
      rescue ArgumentError => e
        raise InvalidType, e.message
      end

      def self.panic_from_map(map)
        Panic.new(
          origin: map["origin"], klass: map["class"], message: map["message"],
          backtrace: map["backtrace"] || [], details: map["details"]
        )
      end
      private_class_method :panic_from_map
    end
  end
end
