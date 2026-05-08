# frozen_string_literal: true

require_relative "group"

module Kobako
  module Service
    # Kobako::Service::Registry — per-Sandbox container of Service Groups
    # (SPEC §B-07..B-11; REFERENCE Ch.6 §Registry 實作要點).
    #
    # Exposes `#define(:GroupName)` to declare or retrieve a {Group}, and
    # `#lookup("GroupName::MemberName")` to resolve a two-level path to the
    # bound object. Guest-driven RPC dispatch goes through {#dispatch}
    # (item #20 will rewire the wasm host import to call it).
    #
    # The Registry is sealed once `Sandbox#run` has been invoked at least
    # once: subsequent `#define` calls raise `ArgumentError`. Per-Group
    # bindings made before the first run remain in effect for all later
    # runs (SPEC §B-07 Notes).
    class Registry
      # Same constant pattern Groups use; kept here for the
      # Group-name validation path.
      NAME_PATTERN = Group::NAME_PATTERN

      def initialize
        @groups = {}
        @sealed = false
      end

      # Declare or retrieve the Group named +name+ (idempotent — B-10).
      #
      # @param name [Symbol, String] constant-form group name.
      # @return [Kobako::Service::Group] the Group instance (same object on
      #   repeat calls — Ruby `equal?`).
      # @raise [ArgumentError] when +name+ is malformed, or when called
      #   after the owning Sandbox has been sealed by `#run`.
      def define(name)
        raise ArgumentError, "cannot define after Sandbox#run has been invoked" if @sealed

        name_str = name.to_s
        unless NAME_PATTERN.match?(name_str)
          raise ArgumentError,
                "GroupName must match #{NAME_PATTERN.inspect} (got #{name.inspect})"
        end

        @groups[name_str] ||= Group.new(name_str)
      end

      # Resolve a `"GroupName::MemberName"` path to the bound Host object.
      #
      # @param target [String] two-level path with `::` separator.
      # @return [Object] the bound Host object.
      # @raise [KeyError] when the group or the member is not bound.
      def lookup(target)
        group_name, member_name = target.to_s.split("::", 2)
        group = @groups[group_name]
        raise KeyError, "no service group named #{group_name.inspect}" if group.nil?
        raise KeyError, "no member #{target.inspect} bound in registry" unless member_name

        group.fetch(member_name)
      end

      # @param target [String] two-level path with `::` separator.
      # @return [Boolean] whether +target+ resolves to a bound member.
      def bound?(target)
        group_name, member_name = target.to_s.split("::", 2)
        return false if member_name.nil?

        group = @groups[group_name]
        !group.nil? && !group[member_name].nil?
      end

      # @return [Array<Kobako::Service::Group>] all declared groups, in
      #   declaration order.
      def groups
        @groups.values
      end

      # @return [Integer] number of declared groups.
      def size
        @groups.size
      end

      # @return [Boolean] whether any groups have been declared.
      def empty?
        @groups.empty?
      end

      # Structured Frame 1 description (REFERENCE Ch.6 §Registry `#guest_preamble`).
      # Returns the unencoded array; msgpack encoding lives at the wire layer.
      #
      # @return [Array<Array(String, Array<String>)>]
      def to_preamble
        @groups.values.map(&:to_preamble)
      end

      # Mark the Registry as sealed. Called by `Sandbox#run` on first run.
      # Idempotent.
      def seal!
        @sealed = true
        self
      end

      # @return [Boolean] whether {#seal!} has been called.
      def sealed?
        @sealed
      end
    end
  end
end
