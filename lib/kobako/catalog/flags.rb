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

        key = Catalog.to_utf8!(name.to_s, "flag name")
        raise ArgumentError, "flag #{key.inspect} already set" if @entries.key?(key)

        @entries[key] = [Catalog.to_utf8!(value, "flag value"), !advisory]
        nil
      end
    end
  end
end
