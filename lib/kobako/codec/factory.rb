# frozen_string_literal: true

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
    # msgpack API — both Encoder and Decoder delegate through it, so
    # the three kobako ext codes (0x00 Symbol, 0x01 Capability Handle,
    # 0x02 Exception envelope) are configured exactly once per instance.
    #
    # One instance is cached per thread: the ext registration is paid once
    # and reused, while the per-operation decode state — ext-envelope nesting
    # depth and whether a Capability Handle was seen — lives in instance
    # variables. This is sound because host codec calls run synchronously on
    # the owning thread, and a nested decode (an ext 0x02 Fault re-entering
    # through its +details+) reuses the same thread instance, so the depth
    # counter accumulates across the re-entry instead of resetting. Class-level
    # +Factory.dump+ / +Factory.load+ / +Factory.reset_handle_tracking!+ /
    # +Factory.saw_handle?+ resolve to the calling thread's instance via
    # +SingleForwardable+.
    class Factory
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

      # An ext 0x02 (Fault) envelope nests through its +details+ field, and
      # each level re-enters the codec with a fresh +MessagePack+ unpacker
      # whose built-in stack guard resets — so ext-envelope depth is tracked
      # on the instance instead. The cap matches the wire's overall nesting
      # bound and keeps a nested chain from exhausting the native stack: an
      # over-deep chain fails as a clean wire error, never a stack-level trap.
      MAX_EXT_DEPTH = 128
      private_constant :MAX_EXT_DEPTH

      # Thread-local slot holding the calling thread's cached Factory.
      FACTORY_KEY = :__kobako_codec_factory__
      private_constant :FACTORY_KEY

      # The calling thread's cached Factory, built on first use so the ext
      # registration is paid once per thread and the per-operation state stays
      # isolated to the thread that runs the codec call.
      def self.instance
        Thread.current[FACTORY_KEY] ||= new
      end
      private_class_method :new

      # Instance-level pass-through onto the wrapped +MessagePack::Factory+.
      # Spelled +def_instance_delegators+ rather than +def_delegators+ because
      # the class also extends +SingleForwardable+ (see the +extend+ block
      # above), which defines its own +def_delegators+ that shadows
      # +Forwardable+'s — the unambiguous forms keep both delegation tiers
      # wired to the right scope.
      def_instance_delegators :@factory, :dump, :load

      # Class-level shortcuts so callers can write +Factory.dump(v)+ instead
      # of +Factory.instance.dump(v)+; each resolves to the calling thread's
      # instance.
      def_single_delegators :instance, :dump, :load, :reset_handle_tracking!, :saw_handle?

      def initialize
        @factory = MessagePack::Factory.new
        @ext_depth = 0
        @saw_handle = false
        register_symbol
        register_handle
        register_fault
      end

      # Clear the decode-time Handle signal ahead of a decode; pair with
      # #saw_handle? read straight after to learn whether the tree carried a
      # Capability Handle. #unpack_handle is the sole chokepoint every Handle
      # passes through, so it records the whole tree in one decode pass and a
      # caller can then skip an all-identity Handle walk when none was present.
      def reset_handle_tracking!
        @saw_handle = false
      end

      # Whether the most recent decode on this thread carried an ext 0x01
      # Handle. Only meaningful immediately after a decode bracketed by
      # #reset_handle_tracking!.
      def saw_handle?
        @saw_handle
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
      # into a wire-layer +InvalidType+ via Codec::Utils.with_boundary.
      # The Value Object owns the id-range contract; this method only
      # owns the frame shape. Records the Handle sighting so a Handle-free
      # decode can skip the downstream resolution walk.
      def unpack_handle(payload)
        @saw_handle = true
        bytes = payload.b
        raise InvalidType, "Handle payload must be 4 bytes, got #{bytes.bytesize}" unless bytes.bytesize == 4

        id = bytes.unpack1("N") # : Integer
        Codec::Utils.with_boundary { Kobako::Handle.restore(id) }
      end

      # Encode the inner ext-0x02 map via Encoder (not +factory.dump+) so
      # the embedded payload flows through the same boundary as a top-level
      # encode — nested kobako values (Handle, nested Fault) reach the
      # registered ext-type packers via the cached instance. A +details+
      # chain nested past MAX_EXT_DEPTH has no wire representation and
      # surfaces as +UnsupportedType+.
      def pack_fault(fault)
        within_ext_frame(UnsupportedType) do
          Encoder.encode("type" => fault.type, "message" => fault.message, "details" => fault.details)
        end
      end

      # Peel the embedded msgpack map and hand it to +Kobako::Fault.new+
      # inside Decoder.decode's block form, so the value-object's
      # +ArgumentError+ invariants surface as +InvalidType+ through the
      # decoder boundary. Inner decode goes through Decoder (not
      # +factory.load+) so the embedded +str+ payloads flow through the
      # same UTF-8 validation as a top-level decode. A nested ext 0x02 in
      # +details+ re-enters this method, so #within_ext_frame bounds the
      # chain depth to keep it from exhausting the native stack.
      def unpack_fault(payload)
        within_ext_frame(InvalidType) do
          Decoder.decode(payload) do |map|
            raise InvalidType, "Fault payload must be a map" unless map.is_a?(Hash)

            Kobako::Fault.new(type: map["type"], message: map["message"], details: map["details"])
          end
        end
      end

      # Track ext-envelope re-entry depth and refuse a chain past
      # MAX_EXT_DEPTH, raising +over_limit+ so the failure lands in the
      # caller's existing wire-error class. The next depth is checked before
      # it is committed, so an over-deep rejection leaves the counter
      # untouched, and the +ensure+ restores the entry value on the way out.
      def within_ext_frame(over_limit)
        depth = @ext_depth + 1
        raise over_limit, "ext envelope nesting exceeds #{MAX_EXT_DEPTH} levels" if depth > MAX_EXT_DEPTH

        @ext_depth = depth
        begin
          yield
        ensure
          @ext_depth = depth - 1
        end
      end
    end
  end
end
