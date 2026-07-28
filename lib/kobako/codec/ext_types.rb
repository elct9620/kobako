# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "utils"
require_relative "state"
require_relative "../handle"

module Kobako
  module Codec
    # The kobako wire ext-type conversions
    # ({docs/wire/payload-msgpack.md}[link:../../../docs/wire/payload-msgpack.md] § Ext Types)
    # as pure functions: per-operation decode state is threaded in as an
    # argument, so the module itself holds nothing. #build_factory assembles
    # the one +MessagePack::Factory+ these conversions are registered on.
    module ExtTypes
      # MessagePack ext type code reserved for Symbol
      # ({docs/wire/payload-msgpack.md}[link:../../../docs/wire/payload-msgpack.md] § Ext Types
      # → ext 0x00). Module-private — mirrors +codec::EXT_SYMBOL+ on the
      # Rust side.
      EXT_SYMBOL = 0x00
      # MessagePack ext type code reserved for Capability Handle
      # ({docs/wire/payload-msgpack.md}[link:../../../docs/wire/payload-msgpack.md] § Ext Types
      # → ext 0x01). Module-private — mirrors +codec::EXT_HANDLE+ on the
      # Rust side.
      EXT_HANDLE = 0x01
      private_constant :EXT_SYMBOL, :EXT_HANDLE

      # Inert ext id the unrepresentable-value guard registers under. It is
      # never emitted (the guard's packer always raises) and never decoded
      # (no unpacker is registered, so the id stays an UnknownExtTypeError on
      # the wire), so it is not a wire ext type: deliberately not named
      # +EXT_*+ like the two real ext codes, since it has no Rust-side mirror
      # and must stay outside the wire-symmetry inventory.
      UNREPRESENTABLE_GUARD_ID = 0x7F
      private_constant :UNREPRESENTABLE_GUARD_ID

      module_function

      # Assemble a +MessagePack::Factory+ with the two kobako ext types plus
      # the unrepresentable-value guard registered, frozen because
      # registration is its only mutation and happens exactly once. The
      # stateful conversions resolve their per-operation state at call time,
      # so one registered factory serves every thread.
      def build_factory
        factory = MessagePack::Factory.new
        register_symbol(factory)
        register_handle(factory)
        register_unrepresentable(factory)
        factory.freeze
      end

      # Symbol-to-name packer for the ext-0x00 registration.
      def pack_symbol(symbol)
        symbol.name
      end

      # Validate the ext-0x00 payload as UTF-8 and intern. Raises
      # InvalidEncodingError on invalid bytes — SPEC forbids the
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
      # into a wire-layer +InvalidTypeError+ via Codec::Utils.with_boundary.
      # The Value Object owns the id-range contract; this method only
      # owns the frame shape. Records the Handle sighting on +state+ so a
      # Handle-free decode can skip the downstream resolution walk.
      def unpack_handle(payload, state)
        state.record_handle!
        bytes = payload.b
        raise InvalidTypeError, "Handle payload must be 4 bytes, got #{bytes.bytesize}" unless bytes.bytesize == 4

        id = bytes.unpack1("N") # : Integer
        Codec::Utils.with_boundary { Kobako::Handle.restore(id) }
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

      # A catch-all packer that rejects any value with no wire representation
      # as +UnsupportedTypeError+. Registered on +BasicObject+ so it also covers
      # BasicObject-based proxies; the narrower Symbol / Handle
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
          packer: ->(_value) { raise UnsupportedTypeError, "value has no wire representation" }
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
