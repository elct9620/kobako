# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "../wire/handle"
require_relative "../wire/exception"
require_relative "factory"

module Kobako
  module Codec
    # Module-level entry point for the host side of the kobako wire
    # (SPEC.md → Wire Codec → Type Mapping).
    #
    # Translates msgpack gem exceptions into the kobako error taxonomy
    # ({Truncated}, {InvalidType}, {InvalidEncoding}, {UnsupportedType}) so
    # callers can pattern-match on the SPEC's wire-violation categories
    # without leaking the gem's internal exception classes.
    #
    # Public API is a single function — {.decode}. The decoder is
    # stateless; the +MessagePack::Unpacker+ instance is built per call
    # because callers always decode exactly one wire value at a time.
    module Decoder
      # The msgpack gem raises these for type/format violations; +ArgumentError+
      # also comes from our ext-type validators (Handle id range, Exception
      # type whitelist). All surface as {InvalidType}.
      INVALID_TYPE_ERRORS = [
        ::MessagePack::UnknownExtTypeError,
        ::MessagePack::MalformedFormatError,
        ::MessagePack::StackError,
        ::ArgumentError
      ].freeze
      private_constant :INVALID_TYPE_ERRORS

      # +UnpackError+ is the gem's umbrella class for short-read / incomplete-buffer
      # faults; +EOFError+ covers underflow at the buffer edge. Both map to {Truncated}.
      TRUNCATED_ERRORS = [::MessagePack::UnpackError, ::EOFError].freeze
      private_constant :TRUNCATED_ERRORS

      # Decode +bytes+ into one Ruby value and validate transitively
      # against the SPEC type mapping. Raises {Truncated}, {InvalidType},
      # or {InvalidEncoding} on wire violations.
      def self.decode(bytes)
        value = Factory.instance.load(bytes.b)
        validate_utf8!(value)
        value
      rescue *INVALID_TYPE_ERRORS => e
        raise InvalidType, e.message
      rescue *TRUNCATED_ERRORS => e
        raise Truncated, e.message
      rescue ::EncodingError => e
        raise InvalidEncoding, e.message
      end

      # SPEC pins +str+ family payloads to UTF-8 (Wire Codec → str/bin
      # Encoding Rules). The msgpack gem returns UTF-8-tagged Strings for
      # str family but does not validate the bytes; +bin+ family decodes
      # to ASCII-8BIT. Walk the tree once and reject invalid UTF-8 in any
      # str-typed leaf. {Kobako::Wire::Exception} payloads are validated
      # transitively: +Factory.unpack_exception+ feeds the inner ext-0x02
      # bytes back through this Decoder, so their +str+ fields are already
      # covered by the time control returns here.
      def self.validate_utf8!(value)
        case value
        when String then validate_string_utf8!(value)
        when Array  then value.each { |v| validate_utf8!(v) }
        when Hash   then value.each_pair { |k, v| validate_pair_utf8!(k, v) }
        end
      end
      private_class_method :validate_utf8!

      def self.validate_string_utf8!(value)
        return unless value.encoding == Encoding::UTF_8
        raise InvalidEncoding, "str payload is not valid UTF-8" unless value.valid_encoding?
      end
      private_class_method :validate_string_utf8!

      def self.validate_pair_utf8!(key, value)
        validate_utf8!(key)
        validate_utf8!(value)
      end
      private_class_method :validate_pair_utf8!
    end
  end
end
