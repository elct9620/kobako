# frozen_string_literal: true

module Kobako
  module Catalog
    class Flags # :nodoc:
      def initialize
        @entries = {} # : Hash[String, [String, bool]]
      end

      # Each entry is the name, the value, and whether the flag is critical.
      def entries
        @entries.map { |name, (value, critical)| [name, value, critical] }
      end

      def register(name, value, advisory:)
        unless name.is_a?(Symbol) || name.is_a?(String)
          raise ArgumentError, "flag name must be a Symbol or String, got #{name.class}"
        end
        raise ArgumentError, "flag value must be a String, got #{value.class}" unless value.is_a?(String)

        key = utf8!(name.to_s, "flag name")
        raise ArgumentError, "flag #{key.inspect} already set" if @entries.key?(key)

        @entries[key] = [utf8!(value, "flag value"), !advisory]
        nil
      end

      private

      # The native side carries a flag only as UTF-8 text, so text read as
      # bytes is re-tagged, and bytes that are not UTF-8 are refused here
      # rather than failing every invocation.
      def utf8!(text, what)
        utf8 = text.dup.force_encoding(Encoding::UTF_8)
        raise ArgumentError, "#{what} must be UTF-8 text" unless utf8.valid_encoding?

        utf8
      end
    end
  end
end
