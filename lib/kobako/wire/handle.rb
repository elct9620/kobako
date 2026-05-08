# frozen_string_literal: true

module Kobako
  module Wire
    # Wire-level value object for an ext-0x01 Capability Handle.
    #
    # SPEC pins the binary layout to fixext 4 with a 4-byte big-endian u32
    # payload (Wire Codec → Ext Types → ext 0x01). ID 0 is reserved as the
    # invalid sentinel; the maximum valid ID is 0x7fff_ffff (2^31 - 1).
    #
    # This is intentionally a thin value object. The runtime-facing
    # +Kobako::Handle+ class lives at a higher layer and may add behaviour
    # (HandleTable bookkeeping, reset semantics). The codec only needs to
    # carry the opaque integer ID across the wire.
    class Handle
      MIN_ID = 1
      MAX_ID = 0x7fff_ffff

      attr_reader :id

      def initialize(id)
        raise ArgumentError, "Handle id must be Integer" unless id.is_a?(Integer)
        raise ArgumentError, "Handle id #{id} out of range [#{MIN_ID}, #{MAX_ID}]" unless id.between?(MIN_ID, MAX_ID)

        @id = id
      end

      def ==(other)
        other.is_a?(Handle) && other.id == @id
      end
      alias eql? ==

      def hash
        [self.class, @id].hash
      end

      def inspect
        "#<Kobako::Wire::Handle id=#{@id}>"
      end
    end
  end
end
