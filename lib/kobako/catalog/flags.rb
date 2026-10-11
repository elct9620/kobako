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

      # The native side carries a value only as UTF-8 text, so a value read
      # as bytes is re-tagged rather than refused.
      def register(name, value, advisory:)
        unless name.is_a?(Symbol) || name.is_a?(String)
          raise ArgumentError, "flag name must be a Symbol or String, got #{name.class}"
        end
        raise ArgumentError, "flag value must be a String, got #{value.class}" unless value.is_a?(String)

        key = name.to_s
        raise ArgumentError, "flag #{key.inspect} already set" if @entries.key?(key)

        @entries[key] = [value.dup.force_encoding(Encoding::UTF_8), !advisory]
        nil
      end
    end
  end
end
