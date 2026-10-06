# frozen_string_literal: true

require_relative "error"

module Kobako
  module Codec
    # Refuses a value nested past the wire's bound before the packer sees it,
    # since the packer walks without a stack guard and a cycle never ends.
    module Nesting # :nodoc:
      # Raise InvalidTypeError when +value+ nests past MAX_NESTING_DEPTH; a
      # value nested exactly to the bound passes.
      def self.assert_within_bound!(value, depth = 0)
        case value
        when ::Array then assert_members_within_bound!(value, depth)
        when ::Hash
          assert_members_within_bound!(value.keys, depth)
          assert_members_within_bound!(value.values, depth)
        end
      end

      # Measure the members of one container at +depth+, each sitting one
      # level deeper. A member is matched by class rather than asked about
      # itself — it may be a BasicObject — and only one that could be a
      # container is walked into.
      def self.assert_members_within_bound!(members, depth)
        return if members.empty?
        if depth >= MAX_NESTING_DEPTH
          raise InvalidTypeError,
                "value nests deeper than #{MAX_NESTING_DEPTH} levels (a reference cycle necessarily does)"
        end
        return unless members.any?(::Enumerable)

        members.grep(::Enumerable) { |member| assert_within_bound!(member, depth + 1) }
      end
      private_class_method :assert_members_within_bound!
    end
  end
end
