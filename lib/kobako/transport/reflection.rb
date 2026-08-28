# frozen_string_literal: true

module Kobako
  module Transport
    # The reflection floor a guest→host dispatch must clear before the
    # Dispatcher reaches +public_send+: which method names on a resolved
    # target count as Service behaviour, and which are Ruby's ambient
    # metaprogramming surface.
    #
    # Answers with a refusal reason rather than raising, so the error
    # taxonomy stays with the Dispatcher and this module holds only the
    # policy.
    module Reflection
      # Modules whose instance methods are ambient Ruby reflection /
      # metaprogramming surface (+send+, +public_send+, +instance_eval+,
      # +method+, +tap+, +instance_variable_get+, ...) rather than Service
      # behaviour. A guest-supplied method name resolving to one of these is
      # rejected: only methods the bound object itself exposes as Service
      # behaviour are reachable, and +public_send(:send, ...)+ would
      # otherwise let a guest pivot through +send+ into the private
      # +Kernel#eval+ / +#system+ surface (host RCE).
      META_OWNERS = [BasicObject, Kernel, Object, Module, Class].freeze
      private_constant :META_OWNERS

      # Callable gadget types whose own public methods are reflection surface
      # (+Proc#binding+ reaches +Binding#eval+, +Method#receiver+ / +#unbind+
      # hand back the underlying object) rather than Service behaviour. Only
      # CALLABLE_ALLOW is reachable on a target of these types; a bound
      # lambda stays invocable, its reflective surface does not.
      GADGET_OWNERS = [Proc, Method, UnboundMethod, Binding].freeze
      private_constant :GADGET_OWNERS

      # The sole methods reachable on a GADGET_OWNERS target: invoking it
      # (+call+ / +[]+ / +yield+) and the harmless +arity+ / +lambda?+
      # describers that aid guest-side debugging.
      CALLABLE_ALLOW = %i[call [] yield arity lambda?].freeze
      private_constant :CALLABLE_ALLOW

      module_function

      # The reason +name+ is unreachable on +target+, or +nil+ when the
      # dispatch may proceed. Composes the ambient-surface floor with the
      # target's own opt-in narrowing, in that order: the predicate only
      # narrows and can never re-open what the floor rejects.
      def refusal(target, name)
        ambient_refusal(target, name) || narrowing_refusal(target, name)
      end

      # Guard against ambient reflection methods. A public method whose
      # owner is a META_OWNERS or GADGET_OWNERS module — or a singleton
      # class, the owner of every class-level method (+File.popen+ /
      # +Kernel.system+, unreachable via any fixed core-module list) — is
      # rejected, except CALLABLE_ALLOW on a gadget target (a bound lambda
      # stays invocable). A name with no concrete public method is allowed
      # only when the target opts into it via +respond_to?+ (dynamic
      # +method_missing+ Services), since the dangerous methods are all
      # concretely defined and therefore never reach that branch.
      def ambient_refusal(target, name)
        owner = target.public_method(name).owner
        return nil unless ambient_owner?(owner, target)
        return nil if GADGET_OWNERS.include?(owner) && CALLABLE_ALLOW.include?(name)

        "method #{name.inspect} is not a Service method"
      rescue NameError
        return nil if target.respond_to?(name)

        "no public method #{name.inspect} on target"
      end

      # Whether +name+'s +owner+ is ambient surface rather than Service
      # behaviour: a core meta module, a callable gadget type, or — when
      # +target+ is itself a Class or Module bound as a Service — the
      # singleton class that owns its class-level API (+File.popen+ /
      # +Kernel.system+, unreachable via any fixed core-module list). A plain
      # object's own singleton method (+def obj.x+) stays reachable, since
      # +target+ is not a Module there.
      def ambient_owner?(owner, target)
        META_OWNERS.include?(owner) ||
          GADGET_OWNERS.include?(owner) ||
          (target.is_a?(Module) && owner.singleton_class?)
      end

      # Consult the target's opt-in narrowing predicate. A bound object may
      # define a private +respond_to_guest?(name)+ to restrict which of its
      # methods the guest reaches; a falsy answer refuses the dispatch. It
      # is consulted with the private surface included so the guest's
      # +public_send+ dispatch can never reach +respond_to_guest?+ itself.
      def narrowing_refusal(target, name)
        return nil unless target.respond_to?(:respond_to_guest?, true)
        return nil if target.__send__(:respond_to_guest?, name)

        "method #{name.inspect} is not exposed to the guest"
      end
    end
  end
end
