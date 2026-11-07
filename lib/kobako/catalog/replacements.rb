# frozen_string_literal: true

module Kobako
  module Catalog
    # The Grants one invocation's Context replaced, each standing in for the
    # Sandbox's Grant at its key.
    class Replacements # :nodoc:
      def initialize(grants)
        @grants = grants
        @replaced = {} # : Hash[Grants::key, Grants::grant]
      end

      # Have +grant+ stand in for the Grant at its key, raising ArgumentError
      # with +refusal+ when the Sandbox holds none there, so a replacement
      # never adds a Grant.
      def replace(grant, refusal)
        raise ArgumentError, refusal unless @grants.include?(grant)

        @replaced[grant.key] = grant
        nil
      end

      # The Sandbox's Grants with each replacement in place, as Grants#entries
      # gives them.
      def entries = @grants.entries(@replaced)
    end
  end
end
