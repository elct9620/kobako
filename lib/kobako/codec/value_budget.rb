# frozen_string_literal: true

require_relative "error"

module Kobako
  module Codec
    # Counts the values a payload holds before the decoder builds any, so a
    # guest cannot make the host build more objects than it could hold in
    # its own memory. Every value takes at least one byte, so a payload no
    # longer than the budget, and holding no float, needs no count.
    #
    # A map key holding a NaN costs one more value for each such key the
    # map already holds, as a Ruby Hash compares it with each: every NaN
    # hashes alike and none is +eql?+ to another.
    #
    # The count stops at the first byte it cannot frame and leaves that
    # payload for the decoder to refuse in its own words.
    module ValueBudget # :nodoc:
      # Marker to the body width of a scalar with no length field.
      SCALARS = {
        0xc0 => 0, 0xc2 => 0, 0xc3 => 0, 0xca => 4, 0xcb => 8,
        0xcc => 1, 0xcd => 2, 0xce => 4, 0xcf => 8,
        0xd0 => 1, 0xd1 => 2, 0xd2 => 4, 0xd3 => 8,
        0xd4 => 2, 0xd5 => 3, 0xd6 => 5, 0xd7 => 9, 0xd8 => 17
      }.freeze
      # Marker to a container's count width and the values each entry holds:
      # two for a map's key and value.
      COUNTS = { 0xdc => [2, 1], 0xdd => [4, 1], 0xde => [2, 2], 0xdf => [4, 2] }.freeze
      # Marker to a str, bin, or ext's length width and type tag width.
      LENGTHS = {
        0xc4 => [1, 0], 0xc5 => [2, 0], 0xc6 => [4, 0],
        0xd9 => [1, 0], 0xda => [2, 0], 0xdb => [4, 0],
        0xc7 => [1, 1], 0xc8 => [2, 1], 0xc9 => [4, 1]
      }.freeze
      UINT = { 1 => "C", 2 => "n", 4 => "N" }.freeze
      private_constant :SCALARS, :COUNTS, :LENGTHS, :UINT

      # A float frame's marker to how its body unpacks.
      FLOAT_FORMATS = { 0xca => "g", 0xcb => "G" }.freeze
      # A float frame's marker; a payload without one holds no NaN.
      FLOATS = Regexp.union(FLOAT_FORMATS.keys.map(&:chr))
      # The fewest bytes a NaN key and its value take: a float 32 frame and
      # a one-byte value.
      NAN_ENTRY = 1 + SCALARS.fetch(0xca) + 1
      private_constant :FLOAT_FORMATS, :FLOATS, :NAN_ENTRY

      def self.assert_within(bytes, budget)
        return if budget.nil? || within_by_size?(bytes, budget)

        Count.new(bytes, budget).run
      end

      # Every value takes at least one byte, so a payload no longer than the
      # budget holds no more values than it, and what its NaN keys could
      # add is bounded by how many of them its bytes could frame.
      def self.within_by_size?(bytes, budget)
        size = bytes.bytesize
        return false if size > budget
        return true unless bytes.match?(FLOATS)

        nan_keys = size / NAN_ENTRY
        size + (nan_keys * (nan_keys - 1) / 2) <= budget
      end

      # One count through +bytes+, walked without recursion so a payload
      # nested past what the decoder allows cannot exhaust the stack first.
      class Count
        # A container still being read: the values it holds directly,
        # whether it is keyed (a map), how many of them were read, how many
        # of its keys held a NaN, and whether anything in it did.
        OpenContainer = Struct.new(:held, :keyed, :read, :nan_keys, :holds_nan)

        def initialize(bytes, budget)
          @bytes = bytes
          @budget = budget
          @remaining = budget
          @open = [] # : Array[OpenContainer]
          @ended = false
        end

        def run
          pos = 0
          loop do
            after, children, keyed = step(pos)
            return if after.nil?

            count_value(pos, children, keyed)
            return if @ended

            pos = after
          end
        end

        private

        # Count the value at +pos+, which holds +children+ directly, keyed
        # when it is a map.
        def count_value(pos, children, keyed)
          spend(1)
          return close_value(nan?(pos)) unless children.positive?

          @open << OpenContainer.new(children, keyed, 0, 0, false)
        end

        # Count a value that ended, and each container it ends, noting once
        # the outermost one did.
        def close_value(holds_nan)
          until @open.empty?
            container = @open.last # : OpenContainer
            charge_key(container) if holds_nan && container.keyed && container.read.even?
            container.holds_nan ||= holds_nan
            container.read += 1
            return unless container.read == container.held

            @open.pop
            holds_nan = container.holds_nan
          end
          @ended = true
        end

        def charge_key(map)
          spend(map.nan_keys)
          map.nan_keys += 1
        end

        def spend(values)
          @remaining -= values
          raise InvalidTypeError, "payload holds more than #{@budget} values" if @remaining.negative?
        end

        def nan?(pos)
          marker = @bytes.getbyte(pos)
          format = FLOAT_FORMATS[marker] or return false
          float = @bytes.byteslice(pos + 1, SCALARS.fetch(marker)).to_s.unpack1(format)
          float.is_a?(Float) && float.nan?
        end

        # The position after the value at +pos+, nil for one the payload
        # cannot frame, the values it holds directly, and whether it is a map.
        def step(pos)
          marker = @bytes.getbyte(pos) or return [nil, 0, false]
          pos += 1
          case marker
          when 0x00..0x7f, 0xe0..0xff then [pos, 0, false]
          when 0x80..0x8f then [pos, (marker & 0x0f) * 2, true]
          when 0x90..0x9f then [pos, marker & 0x0f, false]
          when 0xa0..0xbf then [pos + (marker & 0x1f), 0, false]
          else step_framed(pos, marker)
          end
        end

        def step_framed(pos, marker)
          return [pos + SCALARS.fetch(marker), 0, false] if SCALARS.key?(marker)

          width, per = COUNTS[marker]
          return [pos + width, uint(pos, width) * per, per == 2] if width && per

          width, tag = LENGTHS[marker]
          return [nil, 0, false] unless width && tag

          [pos + width + tag + uint(pos, width), 0, false]
        end

        # A length cut short reads as zero, so the count runs off the end and
        # stops there.
        def uint(pos, width)
          slice = @bytes.byteslice(pos, width).to_s
          return 0 if slice.bytesize < width

          slice.unpack1(UINT.fetch(width)) # : Integer
        end
      end
      private_constant :Count

      private_class_method :within_by_size?
    end
  end
end
