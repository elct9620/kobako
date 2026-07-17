# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "utils"
require_relative "state"
require_relative "../handle"
require_relative "../fault"

module Kobako
  module Codec
    # The kobako wire ext-type conversions
    # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Ext Types)
    # as pure functions: per-operation decode state is threaded in as an
    # argument, so the module itself holds nothing. #build_factory assembles
    # the one +MessagePack::Factory+ these conversions are registered on.
    module ExtTypes
      # MessagePack ext type code reserved for Symbol
      # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Ext Types
      # → ext 0x00). Module-private — mirrors +codec::EXT_SYMBOL+ on the
      # Rust side.
      EXT_SYMBOL = 0x00
      # MessagePack ext type code reserved for Capability Handle
      # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Ext Types
      # → ext 0x01). Module-private — mirrors +codec::EXT_HANDLE+ on the
      # Rust side.
      EXT_HANDLE = 0x01
      # MessagePack ext type code reserved for Exception envelope
      # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Ext Types
      # → ext 0x02). Module-private — mirrors +codec::EXT_ERRENV+ on the
      # Rust side.
      EXT_ERRENV = 0x02
      private_constant :EXT_SYMBOL, :EXT_HANDLE, :EXT_ERRENV

      # Inert ext id the unrepresentable-value guard registers under. It is
      # never emitted (the guard's packer always raises) and never decoded
      # (no unpacker is registered, so the id stays an UnknownExtTypeError on
      # the wire), so it is not a wire ext type: deliberately not named
      # +EXT_*+ like the three real ext codes, since it has no Rust-side mirror
      # and must stay outside the wire-symmetry inventory.
      UNREPRESENTABLE_GUARD_ID = 0x7F
      private_constant :UNREPRESENTABLE_GUARD_ID

      module_function

      # Assemble a +MessagePack::Factory+ with the three kobako ext types plus
      # the unrepresentable-value guard registered, frozen because
      # registration is its only mutation and happens exactly once. The
      # stateful conversions resolve their per-operation state at call time,
      # so one registered factory serves every thread.
      def build_factory
        factory = MessagePack::Factory.new
        register_symbol(factory)
        register_handle(factory)
        register_fault(factory)
        register_unrepresentable(factory)
        factory.freeze
      end

      # Symbol-to-name packer for the ext-0x00 registration.
      def pack_symbol(symbol)
        symbol.name
      end

      # Validate the ext-0x00 payload as UTF-8 and intern. Raises
      # InvalidEncoding on invalid bytes — SPEC forbids the
      # binary-encoding fallback that msgpack-gem's default unpacker
      # would otherwise apply. The re-tag step lives here because the
      # msgpack ext-type unpacker hands us binary bytes; the assertion
      # itself is shared with Decoder via Utils.assert_utf8!. The
      # +"Symbol"+ label keeps the error message in Ruby vocabulary
      # rather than wire-ext-code vocabulary.
      def unpack_symbol(payload)
        name = payload.b.force_encoding(Encoding::UTF_8)
        Utils.assert_utf8!(name, "Symbol payload")
        name.to_sym
      end

      # Handle-id packer for the ext-0x01 registration: the fixext-4
      # big-endian id frame.
      def pack_handle(handle)
        [handle.id].pack("N")
      end

      # Peel off the fixext-4 frame, hand the bytes to the
      # Host-Gem-internal +Kobako::Handle.restore+ factory, and
      # translate the +ArgumentError+ raised by Handle's invariants
      # into a wire-layer +InvalidType+ via Codec::Utils.with_boundary.
      # The Value Object owns the id-range contract; this method only
      # owns the frame shape. Records the Handle sighting on +state+ so a
      # Handle-free decode can skip the downstream resolution walk.
      def unpack_handle(payload, state)
        state.record_handle!
        bytes = payload.b
        raise InvalidType, "Handle payload must be 4 bytes, got #{bytes.bytesize}" unless bytes.bytesize == 4

        id = bytes.unpack1("N") # : Integer
        Codec::Utils.with_boundary { Kobako::Handle.restore(id) }
      end

      # Encode the inner ext-0x02 map via Encoder (not the raw factory) so
      # the embedded payload flows through the same boundary as a top-level
      # encode — nested kobako values (Handle, nested Fault) reach the
      # registered ext-type packers. A +details+ chain nested past the
      # +state+ depth cap has no wire representation and surfaces as
      # +UnsupportedType+. In a payload position (+state+ inside a
      # forbid_faults bracket) the envelope has no wire representation at
      # all, so the refusal routes the value into the position's
      # non-representable handling — the Dispatcher's auto-wrap rescue,
      # or a raise at the yield site.
      def pack_fault(fault, state)
        if state.faults_forbidden?
          raise UnsupportedType, "Kobako::Fault has no wire representation in a payload position"
        end

        state.within_ext_frame(UnsupportedType) do
          Encoder.encode("type" => fault.type, "message" => fault.message, "details" => fault.details)
        end
      end

      # Peel the embedded msgpack map and hand it to +Kobako::Fault.new+
      # inside Decoder.decode's block form, so the value-object's
      # +ArgumentError+ invariants surface as +InvalidType+ through the
      # decoder boundary. Inner decode goes through Decoder (not the raw
      # factory) so the embedded +str+ payloads flow through the same
      # UTF-8 validation as a top-level decode. A nested ext 0x02 in
      # +details+ re-enters this method, so the +state+ ext-frame guard
      # bounds the chain depth to keep it from exhausting the native stack.
      # In a payload position (+state+ inside a forbid_faults bracket) the
      # envelope is a wire violation outright — its sole legal position is
      # the Response fault field.
      def unpack_fault(payload, state)
        if state.faults_forbidden?
          raise InvalidType, "Fault envelope (ext 0x02) is not a legal value in a payload position"
        end

        state.within_ext_frame(InvalidType) do
          Decoder.decode(payload) do |map|
            raise InvalidType, "Fault payload must be a map" unless map.is_a?(Hash)

            Kobako::Fault.new(type: map["type"], message: map["message"], details: map["details"])
          end
        end
      end

      def register_symbol(factory)
        factory.register_type(
          EXT_SYMBOL, Symbol,
          packer: ->(symbol) { ExtTypes.pack_symbol(symbol) },
          unpacker: ->(payload) { ExtTypes.unpack_symbol(payload) }
        )
      end

      def register_handle(factory)
        factory.register_type(
          EXT_HANDLE, Kobako::Handle,
          packer: ->(handle) { ExtTypes.pack_handle(handle) },
          unpacker: ->(payload) { ExtTypes.unpack_handle(payload, State.current) }
        )
      end

      def register_fault(factory)
        factory.register_type(
          EXT_ERRENV, Kobako::Fault,
          packer: ->(fault) { ExtTypes.pack_fault(fault, State.current) },
          unpacker: ->(payload) { ExtTypes.unpack_fault(payload, State.current) }
        )
      end

      # A catch-all packer that rejects any value with no wire representation
      # as +UnsupportedType+. Registered on +BasicObject+ so it also covers
      # BasicObject-based proxies; the narrower Symbol / Handle / Fault
      # registrations still win by most-specific match, and native types never
      # reach it. Packer-only: the guard never writes bytes, so its id is inert
      # and the decode surface stays fail-closed.
      #
      # This makes the host's non-wire detection a positive allowlist — a value
      # outside the type set is rejected here rather than routed to +to_msgpack+
      # — matching the guest's classname allowlist and the Rust codec's closed
      # +Value+ enum. Without it, a value with a permissive +method_missing+
      # answers the codec's +to_msgpack+ probe and mis-encodes as +nil+ instead
      # of crossing as a Capability Handle.
      def register_unrepresentable(factory)
        factory.register_type(
          UNREPRESENTABLE_GUARD_ID, BasicObject,
          packer: ->(_value) { raise UnsupportedType, "value has no wire representation" }
        )
      end
    end

    # The process-wide registered factory: ext registration is paid once at
    # load, and a registered +MessagePack::Factory+ only reads its type
    # registry afterwards, so every thread shares this instance for byte
    # work.
    FACTORY = ExtTypes.build_factory
    private_constant :FACTORY
  end
end
