# frozen_string_literal: true

module Kobako
  module Catalog
    class Grants # :nodoc:
      def initialize
        @entries = {} # : Hash[key, Mount | Network]
      end

      # Each Grant decides what it cannot stand beside.
      def register(grant)
        reason = @entries.each_value.lazy.filter_map { |other| grant.conflict(other) }.first
        raise ArgumentError, reason if reason

        @entries[grant.key] = grant
        nil
      end

      def include?(grant)
        @entries.key?(grant.key)
      end

      # The Mounts, then the network entries, each as the native side reads
      # it, an override standing in for the Grant it shares a key with.
      def entries(overrides = {})
        grants = @entries.merge(overrides).values
        [grants.grep(Mount).map(&:to_native), grants.grep(Network).map(&:to_native)]
      end
    end
  end
end
