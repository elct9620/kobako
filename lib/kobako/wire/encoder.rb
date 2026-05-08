# frozen_string_literal: true

require_relative "error"
require_relative "handle"
require_relative "exception"

module Kobako
  module Wire
    # Pure-Ruby MessagePack encoder restricted to the 11-entry kobako wire
    # type mapping (SPEC.md → Wire Codec → Type Mapping).
    #
    # Independent re-implementation of the Rust guest codec: only the SPEC
    # is consulted. No msgpack gem dependency — uses +String#pack+ and
    # +String#b+ throughout.
    #
    # Usage:
    #   io = String.new(encoding: Encoding::ASCII_8BIT)
    #   Kobako::Wire::Encoder.new(io).write(value)
    #
    # The encoder appends to the given binary-encoded String buffer. Callers
    # may also use +Encoder.encode(value)+ for one-shot encoding into a
    # fresh String.
    class Encoder
      # Single-shot helper.
      def self.encode(value)
        buf = String.new(encoding: Encoding::ASCII_8BIT)
        new(buf).write(value)
        buf
      end

      def initialize(buffer = String.new(encoding: Encoding::ASCII_8BIT))
        @buf = buffer
      end

      attr_reader :buffer

      # Dispatch on Ruby type and append the msgpack-encoded form.
      def write(value)
        case value
        when nil          then write_nil
        when true         then write_true
        when false        then write_false
        when Integer      then write_integer(value)
        when Float        then write_float(value)
        when Handle       then write_handle(value)
        when Exception    then write_exception(value)
        when String       then write_string(value)
        when Array        then write_array(value)
        when Hash         then write_map(value)
        else
          raise UnsupportedType, "no wire encoding for #{value.class}: #{value.inspect}"
        end
        self
      end

      # ---------- primitives ----------

      def write_nil
        @buf << "\xc0".b
      end

      def write_true
        @buf << "\xc3".b
      end

      def write_false
        @buf << "\xc2".b
      end

      # SPEC: msgpack int family — fixint, int 8/16/32/64, uint 8/16/32/64.
      # Pick the narrowest representation, matching standard msgpack practice.
      def write_integer(n)
        if n >= 0
          if n <= 0x7f                  # positive fixint
            @buf << [n].pack("C")
          elsif n <= 0xff               # uint 8
            @buf << "\xcc".b << [n].pack("C")
          elsif n <= 0xffff             # uint 16
            @buf << "\xcd".b << [n].pack("n")
          elsif n <= 0xffff_ffff        # uint 32
            @buf << "\xce".b << [n].pack("N")
          elsif n <= 0xffff_ffff_ffff_ffff # uint 64
            @buf << "\xcf".b << [n].pack("Q>")
          else
            raise UnsupportedType, "integer #{n} exceeds u64 range"
          end
        elsif n >= -32
          @buf << [n].pack("c") # negative fixint
        elsif n >= -0x80              # int 8
          @buf << "\xd0".b << [n].pack("c")
        elsif n >= -0x8000            # int 16
          @buf << "\xd1".b << [n].pack("s>")
        elsif n >= -0x8000_0000       # int 32
          @buf << "\xd2".b << [n].pack("l>")
        elsif n >= -0x8000_0000_0000_0000 # int 64
          @buf << "\xd3".b << [n].pack("q>")
        else
          raise UnsupportedType, "integer #{n} below i64 range"
        end
      end

      # SPEC pins float to msgpack float family. Always use float 64 (f64) —
      # SPEC's wire-side type is f64, and lossy down-conversion to f32 would
      # break round-trip equality on values such as 0.1.
      def write_float(f)
        @buf << "\xcb".b << [f].pack("G")
      end

      # ---------- str / bin ----------

      # SPEC's str/bin Encoding Rules: the msgpack family is selected per
      # the value's Ruby Encoding. UTF-8 with valid byte sequence -> str;
      # any other encoding (notably ASCII-8BIT/BINARY) or invalid UTF-8 -> bin.
      # US-ASCII is treated as str (ASCII is a UTF-8 subset).
      def write_string(s)
        if str_family?(s)
          write_str(s)
        else
          write_bin(s)
        end
      end

      def write_str(s)
        bytes = s.b
        len = bytes.bytesize
        if len <= 31
          @buf << [0xa0 | len].pack("C")
        elsif len <= 0xff
          @buf << "\xd9".b << [len].pack("C")
        elsif len <= 0xffff
          @buf << "\xda".b << [len].pack("n")
        elsif len <= 0xffff_ffff
          @buf << "\xdb".b << [len].pack("N")
        else
          raise UnsupportedType, "str length #{len} exceeds u32"
        end
        @buf << bytes
      end

      def write_bin(s)
        bytes = s.b
        len = bytes.bytesize
        if len <= 0xff
          @buf << "\xc4".b << [len].pack("C")
        elsif len <= 0xffff
          @buf << "\xc5".b << [len].pack("n")
        elsif len <= 0xffff_ffff
          @buf << "\xc6".b << [len].pack("N")
        else
          raise UnsupportedType, "bin length #{len} exceeds u32"
        end
        @buf << bytes
      end

      # ---------- array / map ----------

      def write_array(arr)
        len = arr.length
        if len <= 15
          @buf << [0x90 | len].pack("C")
        elsif len <= 0xffff
          @buf << "\xdc".b << [len].pack("n")
        elsif len <= 0xffff_ffff
          @buf << "\xdd".b << [len].pack("N")
        else
          raise UnsupportedType, "array length #{len} exceeds u32"
        end
        arr.each { |elt| write(elt) }
      end

      def write_map(hash)
        len = hash.size
        if len <= 15
          @buf << [0x80 | len].pack("C")
        elsif len <= 0xffff
          @buf << "\xde".b << [len].pack("n")
        elsif len <= 0xffff_ffff
          @buf << "\xdf".b << [len].pack("N")
        else
          raise UnsupportedType, "map length #{len} exceeds u32"
        end
        hash.each do |k, v|
          write(k)
          write(v)
        end
      end

      # ---------- ext types ----------

      # ext 0x01 Capability Handle: fixext 4 (0xd6 0x01) + big-endian u32.
      def write_handle(handle)
        @buf << "\xd6\x01".b << [handle.id].pack("N")
      end

      # ext 0x02 Exception envelope: variable-length ext wrapping an inner
      # 3-key map. Inner payload encoded first to learn its byte length,
      # then framed as ext 8 / ext 16 / ext 32 per SPEC.
      def write_exception(exc)
        inner = String.new(encoding: Encoding::ASCII_8BIT)
        inner_enc = self.class.new(inner)
        inner_enc.write_map_pairs([
                                    ["type", exc.type],
                                    ["message", exc.message],
                                    ["details", exc.details]
                                  ])
        len = inner.bytesize
        if len <= 0xff
          @buf << "\xc7".b << [len].pack("C") << "\x02".b
        elsif len <= 0xffff
          @buf << "\xc8".b << [len].pack("n") << "\x02".b
        elsif len <= 0xffff_ffff
          @buf << "\xc9".b << [len].pack("N") << "\x02".b
        else
          raise UnsupportedType, "ext payload length #{len} exceeds u32"
        end
        @buf << inner
      end

      # Helper for callers that need to encode a map with deterministic order
      # (used for the Exception envelope inner payload).
      def write_map_pairs(pairs)
        len = pairs.length
        if len <= 15
          @buf << [0x80 | len].pack("C")
        elsif len <= 0xffff
          @buf << "\xde".b << [len].pack("n")
        else
          @buf << "\xdf".b << [len].pack("N")
        end
        pairs.each do |k, v|
          write(k)
          write(v)
        end
      end

      private

      # SPEC str/bin rule: a Ruby String is wire-str iff its encoding is a
      # text encoding (UTF-8 or US-ASCII) AND the bytes are valid in that
      # encoding. Anything else (BINARY/ASCII-8BIT, ISO-8859-1, broken UTF-8)
      # rides the bin family. This matches the SPEC table where +Host Gem
      # Ruby type+ for str is "String (UTF-8 encoding)" and for bin is
      # "String (binary / ASCII-8BIT encoding)".
      def str_family?(s)
        enc = s.encoding
        return false unless [Encoding::UTF_8, Encoding::US_ASCII].include?(enc)

        s.valid_encoding?
      end
    end
  end
end
