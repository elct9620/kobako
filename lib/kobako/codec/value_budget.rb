# frozen_string_literal: true

require_relative "error"

module Kobako
  module Codec
    # Counts the values a payload holds before the decoder builds any, so a
    # guest cannot make the host build more objects than its budget allows
    # (docs/wire/payload-msgpack.md § Value Budget). Every value takes at
    # least one byte, so a payload no longer than the budget needs no count.
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
      # Marker to a container's count width and the values each entry holds.
      COUNTS = { 0xdc => [2, 1], 0xdd => [4, 1], 0xde => [2, 2], 0xdf => [4, 2] }.freeze
      # Marker to a str, bin, or ext's length width and type tag width.
      LENGTHS = {
        0xc4 => [1, 0], 0xc5 => [2, 0], 0xc6 => [4, 0],
        0xd9 => [1, 0], 0xda => [2, 0], 0xdb => [4, 0],
        0xc7 => [1, 1], 0xc8 => [2, 1], 0xc9 => [4, 1]
      }.freeze
      UINT = { 1 => "C", 2 => "n", 4 => "N" }.freeze
      private_constant :SCALARS, :COUNTS, :LENGTHS, :UINT

      def self.assert_within!(bytes, limit)
        return if limit.nil? || bytes.bytesize <= limit

        remaining = limit
        pending = 1
        pos = 0
        while pending.positive?
          pos, children = step(bytes, pos)
          return if pos.nil?
          raise InvalidTypeError, "payload holds more than #{limit} values" if (remaining -= 1).negative?

          pending += children - 1
        end
      end

      # The position after the value at +pos+, nil for one the payload
      # cannot frame, and the values it holds directly.
      def self.step(bytes, pos)
        marker = bytes.getbyte(pos) or return [nil, 0]
        pos += 1
        case marker
        when 0x00..0x7f, 0xe0..0xff then [pos, 0]
        when 0x80..0x8f then [pos, (marker & 0x0f) * 2]
        when 0x90..0x9f then [pos, marker & 0x0f]
        when 0xa0..0xbf then [pos + (marker & 0x1f), 0]
        else framed(bytes, pos, marker)
        end
      end

      def self.framed(bytes, pos, marker)
        return [pos + SCALARS.fetch(marker), 0] if SCALARS.key?(marker)

        width, per = COUNTS[marker]
        return [pos + width, uint(bytes, pos, width) * per] if width && per

        width, tag = LENGTHS[marker]
        return [nil, 0] unless width && tag

        [pos + width + tag + uint(bytes, pos, width), 0]
      end

      # A length cut short reads as zero, so the count runs off the end and
      # stops there.
      def self.uint(bytes, pos, width)
        slice = bytes.byteslice(pos, width).to_s
        return 0 if slice.bytesize < width

        slice.unpack1(UINT.fetch(width)) # : Integer
      end

      private_class_method :step, :framed, :uint
    end
  end
end
