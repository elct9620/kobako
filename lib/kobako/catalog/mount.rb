# frozen_string_literal: true

module Kobako
  module Catalog
    class Mount < Data.define(:at, :access, :filesystem, :policy) # :nodoc:
      ACCESS = %i[read read_write].freeze

      def initialize(at:, access:, filesystem:, policy:)
        raise ArgumentError, "mount path must be a String, got #{at.class}" unless at.is_a?(String)
        raise ArgumentError, "mount path must be an absolute guest path, got #{at.inspect}" if malformed?(at)
        raise ArgumentError, "access must be :read or :read_write, got #{access.inspect}" unless ACCESS.include?(access)
        unless filesystem?(filesystem)
          raise ArgumentError,
                "filesystem must be a Kobako::Filesystem or answer open and stat, got #{filesystem.class}"
        end

        super
      end

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

      # A kind kobako ships, or a Filesystem written in Ruby.
      def filesystem?(filesystem)
        Kobako::Filesystem::KINDS.any? { |kind| filesystem.is_a?(kind) } ||
          (filesystem.respond_to?(:open) && filesystem.respond_to?(:stat))
      end

      # An absolute guest path has no empty, +.+, or +..+ component; the
      # root is +/+ alone.
      def malformed?(path)
        return false if path == "/"

        !path.start_with?("/") || path.delete_prefix("/").split("/", -1).intersect?(["", ".", ".."])
      end

      def nested?(other)
        [at, other].include?("/") || at.start_with?("#{other}/") || other.start_with?("#{at}/")
      end
    end
  end
end
