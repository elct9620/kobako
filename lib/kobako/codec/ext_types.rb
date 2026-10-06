# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "utils"
require_relative "state"
require_relative "../handle"

module Kobako
  module Codec # :nodoc:
    module ExtTypes # :nodoc:
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
      # +EXT_*+ like the two real ext codes, since it has no Rust-side mirror.
      UNREPRESENTABLE_GUARD_ID = 0x7F
      private_constant :UNREPRESENTABLE_GUARD_ID

      module_function

      # The stateful conversions resolve their per-operation state at call
      # time, so one frozen factory serves every thread.
      def build_factory
        factory = MessagePack::Factory.new
        register_symbol(factory)
        register_handle(factory)
        register_unrepresentable(factory)
        factory.freeze
      end

      def pack_symbol(symbol)
        symbol.name
      end

      # Refuses the binary-encoding fallback that msgpack-gem's default
      # unpacker would otherwise apply to invalid bytes.
      def unpack_symbol(payload)
        name = payload.b.force_encoding(Encoding::UTF_8)
        Utils.assert_utf8!(name, "Symbol payload")
        name.to_sym
      end

      def pack_handle(handle)
        [handle.id].pack("N")
      end

      # Handle owns the id-range contract; this method owns only the frame
      # shape. The sighting is recorded so a Handle-free decode can skip the
      # downstream resolution walk.
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

    # Registered once at load and only read afterwards, so every thread
    # shares it.
    FACTORY = ExtTypes.build_factory # :nodoc:
    private_constant :FACTORY
  end
end
