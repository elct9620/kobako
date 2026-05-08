# frozen_string_literal: true

require_relative "error"
require_relative "handle"
require_relative "exception"

module Kobako
  module Wire
    # Pure-Ruby MessagePack decoder restricted to the 11-entry kobako wire
    # type mapping (SPEC.md → Wire Codec → Type Mapping).
    #
    # Independent re-implementation of the Rust guest codec. No msgpack
    # gem dependency.
    #
    # Behaviour highlights pinned by SPEC:
    #   * Unknown msgpack tags  -> InvalidType
    #   * Unknown ext codes     -> InvalidType (only 0x01 / 0x02 are legal)
    #   * Truncated input       -> Truncated
    #   * Invalid UTF-8 in str  -> InvalidEncoding
    #   * Handle ID out of range -> InvalidType (wire violation)
    class Decoder
      def self.decode(bytes)
        new(bytes).read.tap do |_|
          # Trailing-bytes check — kobako payloads are always a single
          # complete value. A second pass should hit EOF.
        end
      end

      def initialize(bytes)
        @buf = bytes.b # work on a binary copy so #byteslice is byte-safe
        @pos = 0
      end

      # Read exactly one wire value and return its Ruby form.
      def read
        tag = read_byte
        decode_with_tag(tag)
      end

      def eof?
        @pos >= @buf.bytesize
      end

      private

      def decode_with_tag(tag)
        # positive fixint  0x00..0x7f
        return tag if tag <= 0x7f
        # negative fixint  0xe0..0xff
        return tag - 0x100 if tag >= 0xe0
        # fixstr           0xa0..0xbf
        return read_str_payload(tag & 0x1f) if (0xa0..0xbf).cover?(tag)
        # fixarray         0x90..0x9f
        return read_array_payload(tag & 0x0f) if (0x90..0x9f).cover?(tag)
        # fixmap           0x80..0x8f
        return read_map_payload(tag & 0x0f) if (0x80..0x8f).cover?(tag)

        case tag
        when 0xc0 then nil
        when 0xc2 then false
        when 0xc3 then true

        when 0xc4 then read_bin_payload(read_uint(1))
        when 0xc5 then read_bin_payload(read_uint(2))
        when 0xc6 then read_bin_payload(read_uint(4))

        when 0xc7 then read_ext_payload(read_uint(1))
        when 0xc8 then read_ext_payload(read_uint(2))
        when 0xc9 then read_ext_payload(read_uint(4))

        when 0xca then read_float32
        when 0xcb then read_float64

        when 0xcc then read_uint(1)
        when 0xcd then read_uint(2)
        when 0xce then read_uint(4)
        when 0xcf then read_uint(8)

        when 0xd0 then read_int(1)
        when 0xd1 then read_int(2)
        when 0xd2 then read_int(4)
        when 0xd3 then read_int(8)

        when 0xd4 then read_fixext(1)
        when 0xd5 then read_fixext(2)
        when 0xd6 then read_fixext(4)
        when 0xd7 then read_fixext(8)
        when 0xd8 then read_fixext(16)

        when 0xd9 then read_str_payload(read_uint(1))
        when 0xda then read_str_payload(read_uint(2))
        when 0xdb then read_str_payload(read_uint(4))

        when 0xdc then read_array_payload(read_uint(2))
        when 0xdd then read_array_payload(read_uint(4))

        when 0xde then read_map_payload(read_uint(2))
        when 0xdf then read_map_payload(read_uint(4))

        else
          # 0xc1 is "never used" in msgpack; any unhandled byte is a wire
          # violation per SPEC's "Any msgpack type or ext code not listed
          # here is a wire violation".
          raise InvalidType, format("unknown msgpack tag 0x%<tag>02x at offset %<pos>d", tag: tag, pos: @pos - 1)
        end
      end

      # ---------- low-level cursor ----------

      def read_byte
        raise Truncated, "expected 1 byte, hit EOF at offset #{@pos}" if @pos >= @buf.bytesize

        b = @buf.getbyte(@pos)
        @pos += 1
        b
      end

      def read_bytes(n)
        avail = @buf.bytesize - @pos
        raise Truncated, "expected #{n} bytes, only #{avail} remain at offset #{@pos}" if avail < n

        slice = @buf.byteslice(@pos, n)
        @pos += n
        slice
      end

      def read_uint(n)
        bytes = read_bytes(n)
        case n
        when 1 then bytes.unpack1("C")
        when 2 then bytes.unpack1("n")
        when 4 then bytes.unpack1("N")
        when 8 then bytes.unpack1("Q>")
        end
      end

      def read_int(n)
        bytes = read_bytes(n)
        case n
        when 1 then bytes.unpack1("c")
        when 2 then bytes.unpack1("s>")
        when 4 then bytes.unpack1("l>")
        when 8 then bytes.unpack1("q>")
        end
      end

      def read_float32
        read_bytes(4).unpack1("g")
      end

      def read_float64
        read_bytes(8).unpack1("G")
      end

      # ---------- str / bin ----------

      def read_str_payload(len)
        bytes = read_bytes(len)
        # SPEC: msgpack str carries UTF-8 text. Force UTF-8 and validate.
        s = bytes.dup.force_encoding(Encoding::UTF_8)
        raise InvalidEncoding, "str payload is not valid UTF-8 at offset #{@pos - len}" unless s.valid_encoding?

        s
      end

      def read_bin_payload(len)
        # SPEC: msgpack bin is a binary byte sequence. Keep ASCII-8BIT.
        read_bytes(len).dup.force_encoding(Encoding::ASCII_8BIT)
      end

      # ---------- array / map ----------

      def read_array_payload(len)
        Array.new(len) { read }
      end

      def read_map_payload(len)
        h = {}
        len.times do
          k = read
          v = read
          h[k] = v
        end
        h
      end

      # ---------- ext ----------

      def read_fixext(payload_len)
        type_byte = read_byte
        payload = read_bytes(payload_len)
        decode_ext(type_byte, payload, payload_len)
      end

      def read_ext_payload(payload_len)
        type_byte = read_byte
        payload = read_bytes(payload_len)
        decode_ext(type_byte, payload, payload_len)
      end

      def decode_ext(type_byte, payload, payload_len)
        case type_byte
        when 0x01
          decode_handle_ext(payload, payload_len)
        when 0x02
          decode_exception_ext(payload)
        else
          raise InvalidType, format("unknown ext type 0x%<code>02x", code: type_byte)
        end
      end

      # SPEC ext 0x01: 4-byte big-endian u32 Handle ID; ID 0 reserved as
      # invalid sentinel; max 0x7fff_ffff. Anything else is a wire violation.
      def decode_handle_ext(payload, payload_len)
        raise InvalidType, "ext 0x01 payload must be 4 bytes, got #{payload_len}" unless payload_len == 4

        id = payload.unpack1("N")
        raise InvalidType, "ext 0x01 Handle id 0 is reserved" if id.zero?
        raise InvalidType, "ext 0x01 Handle id #{id} exceeds max 0x7fff_ffff" if id > Handle::MAX_ID

        Handle.new(id)
      end

      # SPEC ext 0x02: payload is an embedded msgpack map with exactly the
      # three keys "type", "message", "details". We recursively decode the
      # payload and validate the shape.
      def decode_exception_ext(payload)
        inner = self.class.new(payload)
        map = inner.read
        raise InvalidType, "ext 0x02 payload must be a map" unless map.is_a?(Hash)

        type    = map["type"]
        message = map["message"]
        details = map["details"]
        raise InvalidType, "ext 0x02 missing 'type' (str)"    unless type.is_a?(String)
        raise InvalidType, "ext 0x02 missing 'message' (str)" unless message.is_a?(String)
        unless Exception::VALID_TYPES.include?(type)
          raise InvalidType, "ext 0x02 type #{type.inspect} not in #{Exception::VALID_TYPES.inspect}"
        end

        Exception.new(type: type, message: message, details: details)
      end
    end
  end
end
