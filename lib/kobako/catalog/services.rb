# frozen_string_literal: true

require_relative "../constant_name"
require_relative "../errors"
require_relative "../transport/exposure"

module Kobako
  module Catalog
    class Services # :nodoc:
      def initialize
        @bindings = {} # : Hash[String, Kobako::Transport::Exposure]
      end

      # The binding records +object+'s Exposure as it stands now.
      def bind(path, object)
        path_str = validate_path!(path)
        raise ArgumentError, "Service path #{path_str} conflicts with an existing binding" if collision?(path_str)

        @bindings[path_str] = Kobako::Transport::Exposure.of(object)
        self
      end

      def bound?(path)
        @bindings.key?(path.to_s)
      end

      def lookup(target)
        target_str = target.to_s
        raise KeyError, "no service bound at #{target_str.inspect}" unless @bindings.key?(target_str)

        @bindings[target_str]
      end

      # The registry holds the bindings; the wire layout is the native
      # side's.
      def paths
        @bindings.keys
      end

      private

      def validate_path!(path)
        path_str = path.to_s
        segments = path_str.split("::", -1)
        return path_str if !segments.empty? && segments.all? { |seg| CONSTANT_NAME.match?(seg) }

        raise ArgumentError,
              "bind path must be constant-form segments joined by '::' (got #{path.inspect})"
      end

      # Keeps a name from being both a bound Service and a grouping prefix.
      def collision?(path)
        @bindings.each_key.any? do |existing|
          existing == path ||
            existing.start_with?("#{path}::") ||
            path.start_with?("#{existing}::")
        end
      end
    end
  end
end
