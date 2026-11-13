# frozen_string_literal: true

module Kobako
  module Catalog
    class Mount < Data.define(:at, :access, :filesystem, :policy) # :nodoc:
      # The Mount at +at+, refused by the rules an invocation builds it with,
      # so a mistake raises where it was written.
      def initialize(at:, access:, filesystem:, policy:)
        Kobako::Filesystem.validate_mount(at, access, filesystem)
        super
      end

      # The kind and path a Context replaces this Mount at, so a Grant of
      # another kind never meets it.
      def key = [:mount, at]

      # Why +other+ cannot be mounted beside this Mount, or +nil+: a path
      # means one Mount, as Docker refuses a duplicate mount point.
      def conflict(other)
        "path #{at.inspect} is already mounted" if other.key == key
      end

      # The Mount as the native side reads it.
      def to_native = [at, access, filesystem, policy]
    end
  end
end
