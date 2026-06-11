# frozen_string_literal: true

module Kobako
  # A named grouping of Members for one Sandbox
  # ({docs/behavior.md B-07..B-11}[link:../../docs/behavior.md]).
  # Returned by +Sandbox#define+. Each instance owns a flat name→object
  # table of Members; member binding is validated against {NAME_PATTERN}.
  class Namespace
    # Ruby constant-name pattern shared by Namespace and Member names
    # ({docs/behavior.md B-07/B-08 Notes}[link:../../docs/behavior.md]).
    NAME_PATTERN = /\A[A-Z]\w*\z/

    attr_reader :name

    # Build a new Namespace. +name+ is an already-validated Namespace
    # name (must satisfy {NAME_PATTERN}; validation is the caller's
    # responsibility).
    def initialize(name)
      @name = name
      @members = {} # : Hash[String, untyped]
      @sealed = false
    end

    # Bind +object+ under +member+ inside this Namespace. +member+ is a
    # constant-form name as a +Symbol+ or +String+. +object+ is any Ruby
    # object that responds to the methods guest code will invoke. Returns
    # +self+ for chaining. Raises +ArgumentError+ when +member+ does not
    # match the constant pattern, when a Member of the same name is
    # already bound ({docs/behavior.md B-11}[link:../../docs/behavior.md]),
    # or when the owning Sandbox's first invocation has sealed Service
    # registration ({docs/behavior.md E-45}[link:../../docs/behavior.md]).
    def bind(member, object)
      raise ArgumentError, "cannot bind after first Sandbox invocation" if @sealed

      member_str = validate_member_name!(member)
      raise ArgumentError, "Member #{@name}::#{member_str} is already bound" if @members.key?(member_str)

      @members[member_str] = object
      self
    end

    # Mark this Namespace as sealed ({docs/behavior.md B-33}[link:../../docs/behavior.md]).
    # Called by +Kobako::Catalog::Namespaces#seal!+ on the owning
    # Sandbox's first invocation; afterwards {#bind} raises
    # +ArgumentError+ (E-45). Idempotent; returns +self+.
    def seal!
      @sealed = true
      self
    end

    # Member lookup; raises +KeyError+ when no Member is registered
    # under +member+.
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
