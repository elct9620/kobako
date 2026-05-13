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
      # API — both {Encoder} and {Decoder} delegate through it, so the three
      # kobako ext codes (0x00 Symbol, 0x01 Capability Handle, 0x02 Exception
      # envelope) are configured exactly once at module load.
      module Factory
        # MessagePack ext type code reserved for Symbol
        # (SPEC.md → Wire Codec → Ext Types → ext 0x00). Module-private —
        # mirrors +codec::EXT_SYMBOL+ on the Rust side.
        EXT_SYMBOL = 0x00
        # MessagePack ext type code reserved for Capability Handle
        # (SPEC.md → Wire Codec → Ext Types → ext 0x01). Module-private —
        # mirrors +codec::EXT_HANDLE+ on the Rust side.
        EXT_HANDLE = 0x01
        # MessagePack ext type code reserved for Exception envelope
        # (SPEC.md → Wire Codec → Ext Types → ext 0x02). Module-private —
        # mirrors +codec::EXT_ERRENV+ on the Rust side.
        EXT_ERRENV = 0x02
        private_constant :EXT_SYMBOL, :EXT_HANDLE, :EXT_ERRENV

        # Returns the lazily-built process-wide +MessagePack::Factory+.
        def self.instance
          @instance ||= build
        end

        # Build a fresh factory. Exposed for tests that need an isolated
        # instance; production code should call {.instance}.
        def self.build
          factory = MessagePack::Factory.new
          register_symbol_type(factory)
          register_handle_type(factory)
          register_exception_type(factory)
          factory
        end

        def self.register_symbol_type(factory)
          factory.register_type(
            EXT_SYMBOL, Symbol,
            packer: lambda(&:name),
            unpacker: ->(payload) { decode_symbol(payload) }
          )
        end
        private_class_method :register_symbol_type

        # Validate the ext-0x00 payload as UTF-8 and intern. Raises
        # {InvalidEncoding} on invalid bytes — SPEC forbids the
        # binary-encoding fallback that msgpack-gem's default unpacker
        # would otherwise apply.
        def self.decode_symbol(payload)
          name = payload.b.force_encoding(Encoding::UTF_8)
          raise InvalidEncoding, "ext 0x00 payload is not valid UTF-8" unless name.valid_encoding?

          name.to_sym
        end
        private_class_method :decode_symbol

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
            packer: ->(exc) { pack_exception(exc) },
            unpacker: ->(payload) { unpack_exception(payload) }
          )
        end
        private_class_method :register_exception_type

        # Peel off the fixext-4 frame, hand the bytes to +Handle.new+, and
        # translate the +ArgumentError+ raised by Handle's invariants into
        # a wire-layer +InvalidType+ via {Codec.translate_value_object_error}.
        # The Value Object owns the id-range contract; this method only
        # owns the frame shape.
        def self.decode_handle(payload)
          bytes = payload.b
          raise InvalidType, "ext 0x01 payload must be 4 bytes, got #{bytes.bytesize}" unless bytes.bytesize == 4

          Codec.translate_value_object_error { Handle.new(bytes.unpack1("N")) }
        end
        private_class_method :decode_handle

        # Encode the inner ext-0x02 map via {Encoder} (not +factory.dump+) so
        # the embedded payload flows through the same boundary as a top-level
        # encode — nested kobako values (Handle, nested Exception) reach the
        # registered ext-type packers via the cached {.instance}.
        def self.pack_exception(exc)
          Encoder.encode("type" => exc.type, "message" => exc.message, "details" => exc.details)
        end
        private_class_method :pack_exception

        # Peel the embedded msgpack map and hand it to +Exception.new+;
        # translate the value-object's +ArgumentError+ into +InvalidType+
        # at the wire boundary. Inner decode goes through {Decoder} (not
        # +factory.load+) so the embedded +str+ payloads flow through the
        # same UTF-8 validation as a top-level decode.
        #
        # This establishes a runtime cycle Factory → Decoder → Factory:
        # the cached +.instance+ feeds +Decoder.decode+, which re-enters
        # this method when a nested ext 0x02 appears inside +details+. The
        # recursion is bounded by msgpack nesting depth — identical to
        # nested Array / Hash payloads — so no extra guard is needed.
        # Do not switch back to +factory.load+ to "simplify": that path
        # bypasses UTF-8 validation and re-opens the Decoder's special
        # case for Exception (removed in M5).
        def self.unpack_exception(payload)
          map = Decoder.decode(payload)
          raise InvalidType, "ext 0x02 payload must be a map" unless map.is_a?(Hash)

          Codec.translate_value_object_error do
            Exception.new(type: map["type"], message: map["message"], details: map["details"])
          end
        end
        private_class_method :unpack_exception
      end
    end
  end
end
