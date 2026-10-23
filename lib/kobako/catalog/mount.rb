# frozen_string_literal: true

module Kobako
  module Catalog
    class Mount < Data.define(:at, :access, :filesystem, :policy) # :nodoc:
      # The path a Context replaces this Mount at.
      def key = at

      # Why +other+ cannot be mounted beside this Mount, or +nil+: a path
      # means one Mount, and never sits inside another.
      def conflict(other)
        return "path #{at.inspect} is already mounted" if other.at == at

        "path #{at.inspect} nests with the mounted path #{other.at.inspect}" if nested?(other.at)
      end

      # The Mount as the native side reads it.
      def to_native = [at, access, filesystem, policy]

      private

      def nested?(other)
        [at, other].include?("/") || at.start_with?("#{other}/") || other.start_with?("#{at}/")
      end
    end
  end
end
