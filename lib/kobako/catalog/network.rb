# frozen_string_literal: true

module Kobako
  module Catalog
    class Network < Data.define(:name, :entry) # :nodoc:
      # The entry named +name+: +entry+ as it stands, or a Builder adding the
      # block's endpoints to it. Raises ArgumentError for an empty name, which
      # no Context could replace it by.
      def self.of(name, entry)
        name = name.to_s
        raise ArgumentError, "a network entry's name must not be empty" if name.empty?
        return new(name: name.freeze, entry: entry) unless block_given?

        builder = Kobako::Network::Builder.new(entry)
        yield builder
        new(name: name.freeze, entry: builder)
      end

      # The kind and name a Context replaces this entry by, so a Grant of
      # another kind never meets it.
      def key = [:network, name]

      # Why +other+ cannot be given beside this entry, or +nil+: a name means
      # one entry, as a path means one Mount.
      def conflict(other)
        "network entry #{name.inspect} is already given" if other.key == key
      end

      # The entry as the native side reads it.
      def to_native = [name, entry]
    end
  end
end
