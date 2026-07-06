# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "factory"
require_relative "utils"

module Kobako
  module Codec
    # Module-level entry point for the host side of the kobako wire
    # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § Type Mapping).
    #
    # Translates msgpack gem exceptions into the kobako error taxonomy
    # (Truncated, InvalidType, InvalidEncoding, UnsupportedType) so
    # callers can pattern-match on the SPEC's wire-violation categories
    # without leaking the gem's internal exception classes.
    #
    # Public API is a single function — +.decode+. The decoder is
    # stateless; the +MessagePack::Unpacker+ instance is built per call
    # because callers always decode exactly one wire value at a time.
    module Decoder
      # Decode +bytes+ into one Ruby value and validate transitively
      # against the SPEC type mapping. Raises Truncated, InvalidType,
      # or InvalidEncoding on wire violations.
      #
      # When a block is given, the decoded value is yielded and the block's
      # result is returned — wire Value Objects use this to build themselves
      # from the decoded payload. The block runs inside this method's
      # rescue, so a Value Object's +ArgumentError+ invariant failure
      # surfaces as InvalidType without a separate Utils.with_boundary
      # wrapper at the call site.
      def self.decode(bytes)
        value = Factory.load(bytes.b)
        validate_utf8!(value)
        block_given? ? yield(value) : value
      # msgpack gem raises the format/type errors below; +ArgumentError+
      # comes from our ext-type validators (Handle id range, Exception type
      # whitelist) and from a yielded block's Value Object invariants — both
      # are wire violations, so both map to InvalidType.
      rescue ::MessagePack::UnknownExtTypeError, ::MessagePack::MalformedFormatError,
             ::MessagePack::StackError, ::ArgumentError => e
        raise InvalidType, e.message
      # +UnpackError+ is the gem's umbrella class for short-read /
      # incomplete-buffer faults; +EOFError+ covers underflow at the
      # buffer edge.
      rescue ::MessagePack::UnpackError, ::EOFError => e
        raise Truncated, e.message
      end

      # SPEC pins +str+ family payloads to UTF-8
      # ({docs/wire-codec.md}[link:../../../docs/wire-codec.md] § str/bin
      # Encoding Rules). The msgpack gem returns UTF-8-tagged Strings for
      # str family but does not validate the bytes; +bin+ family decodes
      # to ASCII-8BIT. Walk the tree once and reject invalid UTF-8 in any
      # str-typed leaf via Utils.assert_utf8!. Kobako::Fault
      # payloads are validated transitively: +Factory.unpack_fault+
      # feeds the inner ext-0x02 bytes back through this Decoder, so their
      # +str+ fields are already covered by the time control returns here.
      class << self
        private

        def validate_utf8!(value)
          case value
          when String then Utils.assert_utf8!(value, "str payload") if value.encoding == Encoding::UTF_8
          when Array  then value.each { |v| validate_utf8!(v) }
          when Hash
            value.each do |key, val|
              validate_utf8!(key)
              validate_utf8!(val)
            end
          end
        end
      end
    end
  end
end
