# frozen_string_literal: true

require_relative "../encoder"
require_relative "../decoder"
require_relative "../error"

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
          raise ArgumentError, "Panic backtrace must be Array" unless backtrace.is_a?(Array)

          super
        end
      end

      Panic::ORIGIN_SANDBOX = "sandbox"
      Panic::ORIGIN_SERVICE = "service"

      # ---------------- Panic encode / decode ----------------

      def self.encode_panic(panic)
        raise ArgumentError, "encode_panic requires Panic" unless panic.is_a?(Panic)

        buf = String.new(encoding: Encoding::ASCII_8BIT)
        Encoder.new(buf).write_map_pairs(panic_pairs(panic))
        buf
      end

      # SPEC: Panic is a msgpack MAP keyed by name. We always emit the
      # required keys; "backtrace" is emitted only when non-empty (keep
      # the wire compact); "details" only when non-nil. Receivers must
      # ignore unknown keys, so the optional-key absence is wire-legal.
      def self.panic_pairs(panic)
        pairs = [
          ["origin", panic.origin],
          ["class", panic.klass],
          ["message", panic.message]
        ]
        pairs << ["backtrace", panic.backtrace] unless panic.backtrace.empty?
        pairs << ["details", panic.details] unless panic.details.nil?
        pairs
      end
      private_class_method :panic_pairs

      def self.decode_panic(bytes)
        map = Decoder.decode(bytes)
        raise InvalidType, "Panic envelope must be a map, got #{map.class}" unless map.is_a?(Hash)

        origin, klass, message = validate_panic_required_fields!(map)
        backtrace = map["backtrace"] || []
        unless backtrace.is_a?(Array) && backtrace.all?(String)
          raise InvalidType, "Panic backtrace must be array of str"
        end

        Panic.new(origin: origin, klass: klass, message: message,
                  backtrace: backtrace, details: map["details"])
      end

      def self.validate_panic_required_fields!(map)
        origin  = map["origin"]
        klass   = map["class"]
        message = map["message"]
        raise InvalidType, "Panic envelope missing 'origin' (str)"  unless origin.is_a?(String)
        raise InvalidType, "Panic envelope missing 'class' (str)"   unless klass.is_a?(String)
        raise InvalidType, "Panic envelope missing 'message' (str)" unless message.is_a?(String)

        [origin, klass, message]
      end
      private_class_method :validate_panic_required_fields!
    end
  end
end
