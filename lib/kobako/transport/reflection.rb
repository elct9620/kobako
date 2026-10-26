# frozen_string_literal: true

module Kobako
  module Transport
    module Reflection # :nodoc:
      # Callable gadget types whose own public methods are reflection surface
      # (+Proc#binding+ reaches +Binding#eval+, +Method#receiver+ / +#unbind+
      # hand back the underlying object) rather than Service behaviour. Only
      # CALLABLE_ALLOW is reachable on a target of these types; a bound
      # lambda stays invocable, its reflective surface does not.
      GADGET_OWNERS = [Proc, Method, UnboundMethod, Binding].freeze
      private_constant :GADGET_OWNERS

      # The sole methods reachable on a GADGET_OWNERS target: invoking it
      # (+call+ / +[]+ / +yield+) and the harmless +arity+ / +lambda?+
      # describers that aid guest-side debugging. Also the whole of what a
      # callable exposes by default, so the floor and the Exposure agree.
      CALLABLE_ALLOW = %i[call [] yield arity lambda?].freeze

      # The reason +name+ is unreachable through +exposure+, or +nil+ when
      # the dispatch may proceed. Composes the ambient-surface floor with the
      # reference's Exposure, in that order: the Exposure only narrows and
      # can never re-open what the floor rejects.
      def self.refusal(exposure, name)
        ambient_refusal(exposure.object, name) ||
          (exposure.exposes?(name) ? nil : "method #{name.inspect} is not exposed to the guest")
      end

      # Whether +method+ is written in Ruby rather than built into the
      # platform. A native method has no source, and a built-in one names an
      # +<internal:>+ file, so a copy, alias or rebinding of either keeps
      # that mark wherever it is reached from.
      def self.authored?(method)
        file, = method.source_location
        !file.nil? && !file.start_with?("<internal:")
      end

      class << self
        private

        # Guard against ambient reflection methods. A public method whose
        # owner +Class+ inherits or a GADGET_OWNERS module — or a singleton
        # class, the owner of every class-level method (+File.popen+ /
        # +Kernel.system+, unreachable via any fixed core-module list) — is
        # rejected, except CALLABLE_ALLOW on a gadget target (a bound lambda
        # stays invocable). A name with no concrete public method is allowed
        # only when the target opts into it via +respond_to?+ (dynamic
        # +method_missing+ Services), since the dangerous methods are all
        # concretely defined and therefore never reach that branch.
        #
        # +method_missing+ named explicitly is refused whatever its owner:
        # it is Ruby's dynamic-dispatch hook, and a public override (a
        # +Delegator+) binds and calls the private method the guest passes as
        # its first argument (+Kernel#system+). A dynamic +method_missing+
        # Service is untouched — the guest reaches it by the virtual name,
        # which resolves through the +respond_to?+ opt-in, never by naming
        # +method_missing+.
        #
        # A public method must also be written in Ruby, so a Service narrowing
        # itself never reaches a built-in method by a copy, alias or rebinding
        # an owner list cannot see; a non-public one is never reached through
        # a dynamic opt-in either.
        #
        # A name with no method never reaches +instance_method+, which would
        # keep a guest-chosen name as a Symbol for the life of the process.
        def ambient_refusal(target, name)
          return refused(name) if name == :method_missing

          klass = lookup_class(target)
          return defined_refusal(target, name, klass) if klass.public_method_defined?(name)
          return "no public method #{name.inspect} on target" unless target.respond_to?(name)
          return refused(name) if klass.method_defined?(name) || klass.private_method_defined?(name)

          owner_refusal(target, name, klass)
        end

        def defined_refusal(target, name, klass)
          method = klass.instance_method(name)
          owner_refusal(target, name, method.owner) ||
            (written_in_ruby?(target, name, method, klass) ? nil : refused(name))
        end

        # A bound callable keeps its callable names and a record its member
        # readers, though the platform defines both.
        def written_in_ruby?(target, name, method, klass)
          return true if Reflection.authored?(method)
          return true if GADGET_OWNERS.include?(method.owner) && CALLABLE_ALLOW.include?(name)

          (klass <= Struct || klass <= Data) && target.members.include?(name)
        end

        def refused(name)
          "method #{name.inspect} is not a Service method"
        end

        def owner_refusal(target, name, owner)
          return nil unless ambient_owner?(owner, target)
          return nil if GADGET_OWNERS.include?(owner) && CALLABLE_ALLOW.include?(name)

          refused(name)
        end

        # The class a method lookup on +target+ starts at. A plain object's
        # singleton class is taken only when it already holds a public
        # method, since asking for it otherwise would create one.
        def lookup_class(target)
          return target.singleton_class if target.is_a?(Module) || target.singleton_methods.any?

          target.class
        end

        # Whatever +Class+ inherits is surface every object shares rather
        # than Service behaviour: the reflection +BasicObject+ through +Class+
        # define (+send+ pivots into +Kernel#eval+, a host RCE), and every
        # module a library mixes into them, such as a +try+ dispatching by
        # name or a +to_json+ dumping instance variables. A plain object's
        # own singleton method (+def obj.x+) stays reachable, since only a
        # Module target counts its singleton class as ambient.
        def ambient_owner?(owner, target)
          owner >= Class ||
            GADGET_OWNERS.include?(owner) ||
            (target.is_a?(Module) && owner.singleton_class?)
        end
      end
    end
  end
end
