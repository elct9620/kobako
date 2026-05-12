# frozen_string_literal: true

require "msgpack"

require_relative "error"
require_relative "handle"
require_relative "exception"
require_relative "factory"

module Kobako
  module Wire
    # Thin wrapper around +MessagePack::Factory+'s +Unpacker+ for the host
    # side of the kobako wire (SPEC.md → Wire Codec → Type Mapping).
    #
    # Translates msgpack gem exceptions into the kobako error taxonomy
    # ({Truncated}, {InvalidType}, {InvalidEncoding}, {UnsupportedType}) so
    # callers can pattern-match on the SPEC's wire-violation categories
    # without leaking the gem's internal exception classes.
    class Decoder
      # Single-shot helper — decode +bytes+ into one Ruby value.
      def self.decode(bytes)
        new(bytes).read
      end

      def initialize(bytes)
        @buf = bytes.b
        @unpacker = Factory.instance.unpacker
        @unpacker.feed(@buf)
      end

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

      # Read exactly one wire value and return its Ruby form.
      def read
        value = @unpacker.read
        validate_utf8!(value)
        value
      rescue *INVALID_TYPE_ERRORS => e
        raise InvalidType, e.message
      rescue *TRUNCATED_ERRORS => e
        raise Truncated, e.message
      rescue ::EncodingError => e
        raise InvalidEncoding, e.message
      end

      private

      # SPEC pins +str+ family payloads to UTF-8 (Wire Codec → str/bin
      # Encoding Rules). The msgpack gem returns UTF-8-tagged Strings for
      # str family but does not validate the bytes; +bin+ family decodes
      # to ASCII-8BIT. Walk the tree once and reject invalid UTF-8 in any
      # str-typed leaf.
      def validate_utf8!(value)
        case value
        when String    then validate_string_utf8!(value)
        when Array     then value.each { |v| validate_utf8!(v) }
        when Hash      then value.each_pair { |k, v| validate_pair_utf8!(k, v) }
        when Exception then validate_exception_utf8!(value)
        end
      end

      def validate_string_utf8!(value)
        return unless value.encoding == Encoding::UTF_8
        raise InvalidEncoding, "str payload is not valid UTF-8" unless value.valid_encoding?
      end

      def validate_pair_utf8!(key, value)
        validate_utf8!(key)
        validate_utf8!(value)
      end

      def validate_exception_utf8!(exc)
        validate_utf8!(exc.type)
        validate_utf8!(exc.message)
        validate_utf8!(exc.details)
      end
    end
  end
end
