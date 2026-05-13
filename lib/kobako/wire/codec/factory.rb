# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "../handle"
require_relative "../exception"

module Kobako
  module Wire
    module Codec
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
          register_symbol_rejection(factory)
          factory
        end

        def self.register_handle_type(factory)
          factory.register_type(
            EXT_HANDLE, Handle,
            packer: ->(handle) { [handle.id].pack("N") },
            unpacker: ->(payload) { decode_handle(payload) }
          )
        end
        private_class_method :register_handle_type

        def self.register_exception_type(factory)
          factory.register_type(
            EXT_ERRENV, Exception,
            packer: ->(exc) { pack_exception(exc, factory) },
            unpacker: ->(payload) { unpack_exception(payload, factory) }
          )
        end
        private_class_method :register_exception_type

        # Register a packer for Symbol that raises +UnsupportedType+ instead
        # of letting the msgpack gem silently encode it as +str+. Symbols
        # are not in SPEC's 10-entry type-mapping table; the dispatch layer
        # uses this signal to route the value through a Capability Handle
        # ({SPEC.md B-14}[link:../../../SPEC.md]). No ext byte is emitted on
        # the wire because the packer never returns; ext code +0x7f+ is
        # reserved here only so the registration is valid.
        def self.register_symbol_rejection(factory)
          factory.register_type(
            0x7f, Symbol,
            packer: ->(sym) { raise UnsupportedType, "no wire encoding for Symbol: #{sym.inspect}" }
          )
        end
        private_class_method :register_symbol_rejection

        # Peel off the fixext-4 frame, hand the bytes to +Handle.new+, and
        # translate the +ArgumentError+ raised by Handle's invariants into
        # a wire-layer +InvalidType+. The Value Object owns the id-range
        # contract; this method only owns the frame shape.
        def self.decode_handle(payload)
          bytes = payload.b
          raise InvalidType, "ext 0x01 payload must be 4 bytes, got #{bytes.bytesize}" unless bytes.bytesize == 4

          Handle.new(bytes.unpack1("N"))
        rescue ArgumentError => e
          raise InvalidType, e.message
        end
        private_class_method :decode_handle

        def self.pack_exception(exc, factory)
          # Inner payload is itself a msgpack map. We use the *same* factory
          # so any nested kobako values (e.g. Handle in `details`) round-trip
          # through the same ext-type registry.
          factory.dump("type" => exc.type, "message" => exc.message, "details" => exc.details)
        end
        private_class_method :pack_exception

        # Peel the embedded msgpack map and hand it to +Exception.new+;
        # translate the value-object's +ArgumentError+ into +InvalidType+
        # at the wire boundary.
        def self.unpack_exception(payload, factory)
          map = factory.load(payload)
          raise InvalidType, "ext 0x02 payload must be a map" unless map.is_a?(Hash)

          Exception.new(type: map["type"], message: map["message"], details: map["details"])
        rescue ArgumentError => e
          raise InvalidType, e.message
        end
        private_class_method :unpack_exception
      end
    end
  end
end
