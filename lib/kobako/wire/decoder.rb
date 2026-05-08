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

      # Read exactly one wire value and return its Ruby form.
      def read
        value = @unpacker.read
        validate_utf8!(value)
        value
      rescue ::MessagePack::UnknownExtTypeError,
             ::MessagePack::MalformedFormatError,
             ::MessagePack::StackError,
             ::ArgumentError => e
        # +ArgumentError+ comes from our ext-type validators (Handle id
        # range, Exception type whitelist) — surfacing as a wire-violation.
        raise InvalidType, e.message
      rescue ::MessagePack::UnpackError, ::EOFError => e
        # +UnpackError+ is the gem's umbrella class for short-read /
        # incomplete-buffer faults; map to {Truncated}.
        raise Truncated, e.message
      rescue ::EncodingError => e
        raise InvalidEncoding, e.message
      end

      # True if the underlying byte buffer has been fully consumed.
      def eof?
        !@unpacker.buffer.nonempty?
      end

      private

      # SPEC pins +str+ family payloads to UTF-8 (Wire Codec → str/bin
      # Encoding Rules). The msgpack gem returns UTF-8-tagged Strings for
      # str family but does not validate the bytes; +bin+ family decodes
      # to ASCII-8BIT. Walk the tree once and reject invalid UTF-8 in any
      # str-typed leaf.
      def validate_utf8!(value)
        case value
        when String
          return unless value.encoding == Encoding::UTF_8
          raise InvalidEncoding, "str payload is not valid UTF-8" unless value.valid_encoding?
        when Array
          value.each { |v| validate_utf8!(v) }
        when Hash
          value.each do |k, v|
            validate_utf8!(k)
            validate_utf8!(v)
          end
        when Exception
          validate_utf8!(value.type)
          validate_utf8!(value.message)
          validate_utf8!(value.details)
        end
      end
    end
  end
end
