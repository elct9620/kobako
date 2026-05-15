# frozen_string_literal: true

module Kobako
  module RPC
    # A named grouping of Members for one Sandbox
    # ({SPEC.md B-07..B-11}[link:../../../SPEC.md]). Returned by
    # +Sandbox#define+. Each instance owns a flat name→object table of
    # Members; member binding is validated against {NAME_PATTERN}.
    class Namespace
      # Ruby constant-name pattern shared by Namespace and Member names
      # ({SPEC.md B-07/B-08 Notes}[link:../../../SPEC.md]).
      NAME_PATTERN = /\A[A-Z]\w*\z/

      attr_reader :name, :members

      # Build a new Namespace. +name+ is an already-validated Namespace
      # name (must satisfy {NAME_PATTERN}; validation is the caller's
      # responsibility).
      def initialize(name)
        @name = name
        @members = {}
      end

      # Bind +object+ under +member+ inside this Namespace. +member+ is a
      # constant-form name as a +Symbol+ or +String+. +object+ is any Ruby
      # object that responds to the methods guest code will invoke. Returns
      # +self+ for chaining. Raises +ArgumentError+ when +member+ does not
      # match the constant pattern, or a Member of the same name is already
      # bound ({SPEC.md B-11}[link:../../../SPEC.md]).
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

      # Strict variant of {#[]}; raises +KeyError+ when no Member is
      # registered under +member+.
      def fetch(member)
        member_str = member.to_s
        unless @members.key?(member_str)
          raise KeyError,
                "no member named #{member_str.inspect} in namespace #{@name.inspect}"
        end

        @members[member_str]
      end

      # Structured description for the guest preamble (Frame 1). Returns a
      # two-element array +[name, member_keys]+ suitable for msgpack encoding.
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
