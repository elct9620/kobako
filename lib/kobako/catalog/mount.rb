# frozen_string_literal: true

module Kobako
  module Catalog
    class Mount < Data.define(:at, :access, :filesystem, :policy) # :nodoc:
      # The path a Context replaces this Mount at.
      def key = at

      # Why +other+ cannot be mounted beside this Mount, or +nil+: a path
      # means one Mount, as Docker refuses a duplicate mount point.
      def conflict(other)
        "path #{at.inspect} is already mounted" if other.at == at
      end

      # The Mount as the native side reads it.
      def to_native = [at, access, filesystem, policy]
    end
  end
end
