# frozen_string_literal: true

module Kobako
  module Wire
    # Wire-level value object for an ext-0x01 Capability Handle.
    #
    # SPEC pins the binary layout to fixext 4 with a 4-byte big-endian u32
    # payload (Wire Codec → Ext Types → ext 0x01). ID 0 is reserved as the
    # invalid sentinel; the maximum valid ID is 0x7fff_ffff (2^31 - 1).
    #
    # This is intentionally a thin value object built on +Data.define+ so
    # equality, hash, and immutability are inherited. The runtime-facing
    # +Kobako::Handle+ class lives at a higher layer and may add behaviour
    # (HandleTable bookkeeping, reset semantics). The codec only needs to
    # carry the opaque integer ID across the wire.
    Handle = Data.define(:id) do
      def initialize(id:)
        min = self.class::MIN_ID
        max = self.class::MAX_ID
        raise ArgumentError, "Handle id must be Integer" unless id.is_a?(Integer)
        raise ArgumentError, "Handle id #{id} out of range [#{min}, #{max}]" unless id.between?(min, max)

        super
      end
    end

    Handle::MIN_ID = 1
    Handle::MAX_ID = 0x7fff_ffff
  end
end
