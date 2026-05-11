# frozen_string_literal: true

module Kobako
  class Registry
    # ===========================================================================
    # Internal class: ServiceGroup
    #
    # A named namespace of Service Members for one Sandbox ({SPEC.md §B-07..B-11}[link:../../../SPEC.md]).
    # ===========================================================================
    class ServiceGroup
      # Ruby constant-name pattern ({SPEC.md §B-07/B-08 Notes}[link:../../../SPEC.md]).
      NAME_PATTERN = /\A[A-Z]\w*\z/

      attr_reader :name, :members

      # @param name [String] already-validated Group name.
      def initialize(name)
        @name = name
        @members = {}
      end

      # Bind +object+ under +member+ inside this group.
      #
      # @param member [Symbol, String] constant-form member name.
      # @param object [Object] any object responding to the methods guest code
      #   will invoke.
      # @return [self] for chaining.
      # @raise [ArgumentError] when +member+ does not match the constant
      #   pattern, or a member of the same name is already bound ({SPEC.md §B-11}[link:../../../SPEC.md]).
      def bind(member, object)
        member_str = validate_member_name!(member)
        raise ArgumentError, "Member #{@name}::#{member_str} is already bound" if @members.key?(member_str)

        @members[member_str] = object
        self
      end

      # Member lookup. Returns the bound object or +nil+ when missing.
      def [](member)
        @members[member.to_s]
      end

      # Strict variant of {#[]}; raises when the member is unbound.
      #
      # @raise [KeyError] when no member is registered under +member+.
      def fetch(member)
        member_str = member.to_s
        unless @members.key?(member_str)
          raise KeyError, "no member named #{member_str.inspect} in group #{@name.inspect}"
        end

        @members[member_str]
      end

      # Structured description for the guest preamble (Frame 1).
      #
      # @return [Array(String, Array<String>)]
      def to_preamble
        [@name, @members.keys]
      end

      private

      def validate_member_name!(member)
        member_str = member.to_s
        unless NAME_PATTERN.match?(member_str)
          raise ArgumentError,
                "MemberName must match #{NAME_PATTERN.inspect} (got #{member.inspect})"
        end

        member_str
      end
    end
  end
end
