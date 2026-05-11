# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "handle"
require_relative "exception"

module Kobako
  module Wire
    # Cached +MessagePack::Factory+ that owns the kobako wire ext-type
    # registration (SPEC.md → Wire Codec → Ext Types).
    #
    # The factory is the single place in the host gem that touches msgpack
    # API — both {Encoder} and {Decoder} delegate through it, so the ext
    # codes 0x01 (Capability Handle) and 0x02 (Exception envelope) are
    # configured exactly once at module load.
    module Factory
      # Returns the lazily-built process-wide +MessagePack::Factory+.
      def self.instance
        @instance ||= build
      end

      # Build a fresh factory. Exposed for tests that need an isolated
      # instance; production code should call {.instance}.
      def self.build
        factory = MessagePack::Factory.new
        register_handle_type(factory)
        register_exception_type(factory)
        factory
      end

      def self.register_handle_type(factory)
        factory.register_type(
          0x01, Handle,
          packer: ->(handle) { [handle.id].pack("N") },
          unpacker: ->(payload) { decode_handle(payload) }
        )
      end
      private_class_method :register_handle_type

      def self.register_exception_type(factory)
        factory.register_type(
          0x02, Exception,
          packer: ->(exc) { pack_exception(exc, factory) },
          unpacker: ->(payload) { unpack_exception(payload, factory) }
        )
      end
      private_class_method :register_exception_type

      # Pre-validates the payload here so failures surface as +InvalidType+
      # (wire-error taxonomy) rather than the +ArgumentError+ that
      # +Handle#initialize+ would raise — the duplicate checks are intentional.
      def self.decode_handle(payload)
        bytes = payload.b
        raise InvalidType, "ext 0x01 payload must be 4 bytes, got #{bytes.bytesize}" unless bytes.bytesize == 4

        id = bytes.unpack1("N")
        raise InvalidType, "ext 0x01 Handle id 0 is reserved" if id.zero?
        raise InvalidType, "ext 0x01 Handle id #{id} exceeds max #{Handle::MAX_ID}" if id > Handle::MAX_ID

        Handle.new(id)
      end
      private_class_method :decode_handle

      def self.pack_exception(exc, factory)
        # Inner payload is itself a msgpack map. We use the *same* factory
        # so any nested kobako values (e.g. Handle in `details`) round-trip
        # through the same ext-type registry.
        factory.dump("type" => exc.type, "message" => exc.message, "details" => exc.details)
      end
      private_class_method :pack_exception

      def self.unpack_exception(payload, factory)
        map = factory.load(payload)
        raise InvalidType, "ext 0x02 payload must be a map" unless map.is_a?(Hash)

        type, message = validate_exception_map!(map)
        Exception.new(type: type, message: message, details: map["details"])
      end
      private_class_method :unpack_exception

      # Pre-validates map keys and type here so failures surface as +InvalidType+
      # rather than the +ArgumentError+ that +Exception#initialize+ would raise —
      # the duplicate checks are intentional.
      def self.validate_exception_map!(map)
        type    = map["type"]
        message = map["message"]
        raise InvalidType, "ext 0x02 missing 'type' (str)"    unless type.is_a?(String)
        raise InvalidType, "ext 0x02 missing 'message' (str)" unless message.is_a?(String)
        unless Exception::VALID_TYPES.include?(type)
          raise InvalidType, "ext 0x02 type #{type.inspect} not in #{Exception::VALID_TYPES.inspect}"
        end

        [type, message]
      end
      private_class_method :validate_exception_map!
    end
  end
end
