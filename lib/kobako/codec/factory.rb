# frozen_string_literal: true

require "singleton"
require "forwardable"
require "msgpack"

require_relative "error"
require_relative "utils"
require_relative "../rpc/handle"
require_relative "../wire/exception"

module Kobako
  module Codec
    # Cached +MessagePack::Factory+ that owns the kobako wire ext-type
    # registration (SPEC.md → Wire Codec → Ext Types).
    #
    # The factory is the single place in the host gem that touches the
    # msgpack API — both {Encoder} and {Decoder} delegate through it, so
    # the three kobako ext codes (0x00 Symbol, 0x01 Capability Handle,
    # 0x02 Exception envelope) are configured exactly once at first use.
    #
    # Lifecycle is owned by +Singleton+ from the Ruby standard library:
    # +Factory.instance+ is lazy, thread-safe, and process-wide. Class-level
    # +Factory.dump+ / +Factory.load+ shortcuts are exposed via
    # +SingleForwardable+ so callers do not have to spell the +.instance+
    # hop at every call site; the instance-level +#dump+ / +#load+ are in
    # turn delegated to the wrapped +MessagePack::Factory+ via +Forwardable+.
    class Factory
      include Singleton
      extend Forwardable
      extend SingleForwardable

      # MessagePack ext type code reserved for Symbol
      # (SPEC.md → Wire Codec → Ext Types → ext 0x00). Class-private —
      # mirrors +codec::EXT_SYMBOL+ on the Rust side.
      EXT_SYMBOL = 0x00
      # MessagePack ext type code reserved for Capability Handle
      # (SPEC.md → Wire Codec → Ext Types → ext 0x01). Class-private —
      # mirrors +codec::EXT_HANDLE+ on the Rust side.
      EXT_HANDLE = 0x01
      # MessagePack ext type code reserved for Exception envelope
      # (SPEC.md → Wire Codec → Ext Types → ext 0x02). Class-private —
      # mirrors +codec::EXT_ERRENV+ on the Rust side.
      EXT_ERRENV = 0x02
      private_constant :EXT_SYMBOL, :EXT_HANDLE, :EXT_ERRENV

      # Instance-level pass-through onto the wrapped +MessagePack::Factory+.
      # Spelled +def_instance_delegators+ rather than +def_delegators+ because
      # the class also extends +SingleForwardable+ (see the +extend+ block
      # above), which defines its own +def_delegators+ that shadows
      # +Forwardable+'s — the unambiguous forms keep both delegation tiers
      # wired to the right scope.
      def_instance_delegators :@factory, :dump, :load

      # Class-level shortcuts so callers can write +Factory.dump(v)+ instead
      # of +Factory.instance.dump(v)+; both resolve to the same singleton.
      def_single_delegators :instance, :dump, :load

      def initialize
        @factory = MessagePack::Factory.new
        register_symbol_type
        register_handle_type
        register_exception_type
      end

      private

      def register_symbol_type
        @factory.register_type(
          EXT_SYMBOL, Symbol,
          packer: method(:pack_symbol),
          unpacker: method(:decode_symbol)
        )
      end

      # Symbol-to-name packer — extracted to a real method so Steep can
      # resolve the proc shape without tripping on +lambda(&:name)+'s
      # +Symbol#to_proc+ inference path.
      def pack_symbol(symbol)
        symbol.name
      end

      # Validate the ext-0x00 payload as UTF-8 and intern. Raises
      # {InvalidEncoding} on invalid bytes — SPEC forbids the
      # binary-encoding fallback that msgpack-gem's default unpacker
      # would otherwise apply. The re-tag step lives here because the
      # msgpack ext-type unpacker hands us binary bytes; the assertion
      # itself is shared with {Decoder} via {Utils.assert_utf8!}.
      def decode_symbol(payload)
        name = payload.b.force_encoding(Encoding::UTF_8)
        Utils.assert_utf8!(name, "ext 0x00 payload")
        name.to_sym
      end

      def register_handle_type
        @factory.register_type(
          EXT_HANDLE, RPC::Handle,
          packer: ->(handle) { [handle.id].pack("N") },
          unpacker: ->(payload) { decode_handle(payload) }
        )
      end

      def register_exception_type
        @factory.register_type(
          EXT_ERRENV, Wire::Exception,
          packer: ->(exc) { pack_exception(exc) },
          unpacker: ->(payload) { unpack_exception(payload) }
        )
      end

      # Peel off the fixext-4 frame, hand the bytes to +RPC::Handle.new+, and
      # translate the +ArgumentError+ raised by Handle's invariants into
      # a wire-layer +InvalidType+ via {Codec::Utils.translate_value_object_error}.
      # The Value Object owns the id-range contract; this method only
      # owns the frame shape.
      def decode_handle(payload)
        bytes = payload.b
        raise InvalidType, "ext 0x01 payload must be 4 bytes, got #{bytes.bytesize}" unless bytes.bytesize == 4

        id = bytes.unpack1("N") # : Integer
        Codec::Utils.translate_value_object_error { RPC::Handle.new(id) }
      end

      # Encode the inner ext-0x02 map via {Encoder} (not +factory.dump+) so
      # the embedded payload flows through the same boundary as a top-level
      # encode — nested kobako values (Handle, nested Exception) reach the
      # registered ext-type packers via the cached singleton.
      def pack_exception(exc)
        Encoder.encode("type" => exc.type, "message" => exc.message, "details" => exc.details)
      end

      # Peel the embedded msgpack map and hand it to +Wire::Exception.new+;
      # translate the value-object's +ArgumentError+ into +InvalidType+
      # at the wire boundary. Inner decode goes through {Decoder} (not
      # +factory.load+) so the embedded +str+ payloads flow through the
      # same UTF-8 validation as a top-level decode.
      #
      # This establishes a runtime cycle Factory → Decoder → Factory: the
      # singleton instance feeds +Decoder.decode+, which re-enters this
      # method when a nested ext 0x02 appears inside +details+. The recursion
      # is bounded by msgpack nesting depth — identical to nested Array /
      # Hash payloads — so no extra guard is needed. Do not switch back to
      # +factory.load+ to "simplify": that path bypasses UTF-8 validation
      # and re-opens the Decoder's special case for Exception (removed in M5).
      def unpack_exception(payload)
        map = Decoder.decode(payload)
        raise InvalidType, "ext 0x02 payload must be a map" unless map.is_a?(Hash)

        Codec::Utils.translate_value_object_error do
          Wire::Exception.new(type: map["type"], message: map["message"], details: map["details"])
        end
      end
    end
  end
end
