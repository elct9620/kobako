# frozen_string_literal: true

require "singleton"
require "forwardable"
require "msgpack"

require_relative "error"
require_relative "utils"
require_relative "../handle"
require_relative "../fault"

module Kobako
  module Codec
    # Cached +MessagePack::Factory+ that owns the kobako wire ext-type
    # registration ({docs/wire-codec.md}[link:../../../docs/wire-codec.md]
    # § Ext Types).
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
      # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Ext Types
      # → ext 0x00). Class-private — mirrors +codec::EXT_SYMBOL+ on the
      # Rust side.
      EXT_SYMBOL = 0x00
      # MessagePack ext type code reserved for Capability Handle
      # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Ext Types
      # → ext 0x01). Class-private — mirrors +codec::EXT_HANDLE+ on the
      # Rust side.
      EXT_HANDLE = 0x01
      # MessagePack ext type code reserved for Exception envelope
      # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Ext Types
      # → ext 0x02). Class-private — mirrors +codec::EXT_ERRENV+ on the
      # Rust side.
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
        register_symbol
        register_handle
        register_fault
      end

      private

      def register_symbol
        @factory.register_type(
          EXT_SYMBOL, Symbol,
          packer: method(:pack_symbol),
          unpacker: method(:unpack_symbol)
        )
      end

      # Symbol-to-name packer for the ext-0x00 registration.
      def pack_symbol(symbol)
        symbol.name
      end

      # Validate the ext-0x00 payload as UTF-8 and intern. Raises
      # {InvalidEncoding} on invalid bytes — SPEC forbids the
      # binary-encoding fallback that msgpack-gem's default unpacker
      # would otherwise apply. The re-tag step lives here because the
      # msgpack ext-type unpacker hands us binary bytes; the assertion
      # itself is shared with {Decoder} via {Utils.assert_utf8!}. The
      # +"Symbol"+ label keeps the error message in Ruby vocabulary
      # rather than wire-ext-code vocabulary.
      def unpack_symbol(payload)
        name = payload.b.force_encoding(Encoding::UTF_8)
        Utils.assert_utf8!(name, "Symbol payload")
        name.to_sym
      end

      def register_handle
        @factory.register_type(
          EXT_HANDLE, Kobako::Handle,
          packer: ->(handle) { [handle.id].pack("N") },
          unpacker: ->(payload) { unpack_handle(payload) }
        )
      end

      def register_fault
        @factory.register_type(
          EXT_ERRENV, Kobako::Fault,
          packer: ->(fault) { pack_fault(fault) },
          unpacker: ->(payload) { unpack_fault(payload) }
        )
      end

      # Peel off the fixext-4 frame, hand the bytes to the
      # Host-Gem-internal +Kobako::Handle.restore+ factory, and
      # translate the +ArgumentError+ raised by Handle's invariants
      # into a wire-layer +InvalidType+ via {Codec::Utils.with_boundary}.
      # The Value Object owns the id-range contract; this method only
      # owns the frame shape.
      def unpack_handle(payload)
        bytes = payload.b
        raise InvalidType, "Handle payload must be 4 bytes, got #{bytes.bytesize}" unless bytes.bytesize == 4

        id = bytes.unpack1("N") # : Integer
        Codec::Utils.with_boundary { Kobako::Handle.restore(id) }
      end

      # Encode the inner ext-0x02 map via {Encoder} (not +factory.dump+) so
      # the embedded payload flows through the same boundary as a top-level
      # encode — nested kobako values (Handle, nested Fault) reach the
      # registered ext-type packers via the cached singleton.
      def pack_fault(fault)
        Encoder.encode("type" => fault.type, "message" => fault.message, "details" => fault.details)
      end

      # Peel the embedded msgpack map and hand it to +Kobako::Fault.new+
      # inside {Decoder.decode}'s block form, so the value-object's
      # +ArgumentError+ invariants surface as +InvalidType+ through the
      # decoder boundary. Inner decode goes through {Decoder} (not
      # +factory.load+) so the embedded +str+ payloads flow through the
      # same UTF-8 validation as a top-level decode.
      #
      # This establishes a runtime cycle Factory → Decoder → Factory: the
      # singleton instance feeds +Decoder.decode+, which re-enters this
      # method when a nested ext 0x02 appears inside +details+. The recursion
      # is bounded by msgpack nesting depth — identical to nested Array /
      # Hash payloads — so no extra guard is needed. Do not switch back to
      # +factory.load+ to "simplify": that path bypasses UTF-8 validation.
      def unpack_fault(payload)
        Decoder.decode(payload) do |map|
          raise InvalidType, "Fault payload must be a map" unless map.is_a?(Hash)

          Kobako::Fault.new(type: map["type"], message: map["message"], details: map["details"])
        end
      end
    end
  end
end
