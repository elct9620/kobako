# frozen_string_literal: true

require_relative "../codec"
require_relative "request"
require_relative "response"
require_relative "yield"
require_relative "yielder"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns the pure-function dispatcher that decodes guest-initiated
  # Requests and produces encoded Responses.
  module Transport
    # Pure-function dispatcher for guest-initiated transport calls.
    # Decodes a msgpack-encoded Request envelope, resolves the target
    # object through the Catalog::Namespaces (path lookup) or
    # Catalog::Handles (Handle lookup), invokes the method, and returns
    # a msgpack-encoded Response envelope.
    #
    # The module is stateless — all mutable state is threaded through
    # arguments so Dispatcher has no instance variables and no side
    # effects beyond mutating the Catalog::Handles via +alloc+ when a
    # non-wire-representable return value must be wrapped.
    #
    # Entry point:
    #
    #   Kobako::Transport::Dispatcher.dispatch(request_bytes, namespaces, handler, yield_to_guest)
    #   # => msgpack-encoded Response bytes (never raises)
    module Dispatcher
      # Throw tag for the {Yielder}'s break unwind back to the
      # dispatcher's +catch+ frame. +private_constant+ is a
      # convention boundary — not a defence.
      BREAK_THROW = :__kobako_break__
      private_constant :BREAK_THROW

      module_function

      # Internal sentinel raised when target resolution fails. Mapped to
      # Response.error with type="undefined". Contained at the wire boundary —
      # not part of the public Kobako error taxonomy.
      class UndefinedTargetError < StandardError; end

      # Modules whose instance methods are ambient Ruby reflection /
      # metaprogramming surface (+send+, +public_send+, +instance_eval+,
      # +method+, +tap+, +instance_variable_get+, ...) rather than Service
      # behaviour. A guest-supplied method name resolving to one of these is
      # rejected: only methods the bound object itself exposes as Service behaviour are
      # reachable, and +public_send(:send, ...)+ would otherwise let a guest
      # pivot through +send+ into the private +Kernel#eval+ / +#system+
      # surface (host RCE).
      META_OWNERS = [BasicObject, Kernel, Object, Module, Class].freeze
      private_constant :META_OWNERS

      # Callable gadget types whose own public methods are reflection surface
      # (+Proc#binding+ reaches +Binding#eval+, +Method#receiver+ / +#unbind+
      # hand back the underlying object) rather than Service behaviour. Only
      # {CALLABLE_ALLOW} is reachable on a target of these types; a bound
      # lambda stays invocable, its reflective surface does not.
      GADGET_OWNERS = [Proc, Method, UnboundMethod, Binding].freeze
      private_constant :GADGET_OWNERS

      # The sole methods reachable on a {GADGET_OWNERS} target: invoking it
      # (+call+ / +[]+ / +yield+) and the harmless +arity+ / +lambda?+
      # describers that aid guest-side debugging.
      CALLABLE_ALLOW = %i[call [] yield arity lambda?].freeze
      private_constant :CALLABLE_ALLOW

      # Dispatch a single transport request and return the encoded
      # Response bytes. Invoked from the +Runtime#on_dispatch+ Proc that
      # +Kobako::Sandbox#initialize+ installs on the ext side; +namespaces+,
      # +handler+, and +yield_to_guest+ are captured in that Proc's
      # closure so the Dispatcher stays stateless and the registry doesn't
      # need to publish accessors for the Sandbox-owned +Catalog::Handles+
      # or +Runtime+. +yield_to_guest+ is a +String → String+ callable
      # (typically +Runtime#yield_to_active_invocation+ bound as a lambda)
      # used only when the Request carries +block_given: true+. Always
      # returns a binary String — every failure path is reified as a
      # Response.error envelope so the guest sees a transport error rather
      # than a wasm trap.
      def dispatch(request_bytes, namespaces, handler, yield_to_guest)
        request = Kobako::Transport::Request.decode(request_bytes)
        target = resolve_target(request.target, namespaces, handler)
        args, kwargs = resolve_call_args(request, handler)
        yielder = Yielder.new(yield_to_guest, BREAK_THROW, handler) if request.block_given
        value = catch(BREAK_THROW) { invoke(target, request.method_name, args, kwargs, yielder) }
        encode_ok(value, handler)
      rescue StandardError => e
        encode_caught_error(e)
      ensure
        yielder&.invalidate!
      end

      # Resolve positional and keyword arguments off +request+ in one
      # step. Both pass through {#resolve_arg} so Capability Handles
      # round-trip back to the host-side Ruby object before the call
      # reaches +public_send+.
      def resolve_call_args(request, handler)
        [request.args.map { |v| resolve_arg(v, handler) },
         request.kwargs.transform_values { |v| resolve_arg(v, handler) }]
      end

      # Map an error caught at the dispatch boundary to a +Response.error+
      # envelope (binary msgpack). +error+ is the +StandardError+ caught by
      # {#dispatch}'s rescue; the +type+ field tells the guest which kind
      # of failure it was so it can raise the matching proxy-side error.
      def encode_caught_error(error)
        case error
        when Kobako::Codec::Error then encode_error("runtime",
                                                    "Sandbox received a malformed request: #{error.message}")
        when UndefinedTargetError then encode_error("undefined", error.message)
        when ArgumentError        then encode_error("argument", error.message)
        else                           encode_error("runtime", "#{error.class}: #{error.message}")
        end
      end

      # Dispatch +method+ on +target+. +kwargs+ is already Symbol-keyed
      # (the +Request+ invariant pins it). The empty-kwargs branch omits
      # the +**+ splat so Ruby 3.x's strict kwargs separation does not
      # reject calls to no-kwarg methods when the wire carries the
      # uniform empty-map shape.
      #
      # +yielder+ is the host-side {Yielder} materialised when the guest
      # call site supplied a block; its {Yielder#to_proc}
      # rides the +&block+ slot. +&nil+ is a no-op block argument in Ruby,
      # so the same call site handles both cases without an explicit
      # conditional.
      def invoke(target, method, args, kwargs, yielder = nil)
        name = method.to_sym
        reject_meta_method!(target, name)
        reject_unexposed!(target, name)
        block = yielder&.to_proc
        if kwargs.empty?
          target.public_send(name, *args, &block)
        else
          target.public_send(name, *args, **kwargs, &block)
        end
      end

      # Guard the +public_send+ below against ambient reflection methods.
      # A public method whose owner is a {META_OWNERS} or {GADGET_OWNERS} module is
      # rejected, except {CALLABLE_ALLOW} on a gadget target (a bound lambda
      # stays invocable). A name with no concrete public method is allowed
      # only when the target opts into it via +respond_to?+ (dynamic
      # +method_missing+ Services), since the dangerous methods are all
      # concretely defined and therefore never reach that branch.
      def reject_meta_method!(target, name)
        owner = target.public_method(name).owner
        gadget = GADGET_OWNERS.include?(owner)
        return unless META_OWNERS.include?(owner) || gadget
        return if gadget && CALLABLE_ALLOW.include?(name)

        raise UndefinedTargetError, "method #{name.inspect} is not a Service method"
      rescue NameError
        return if target.respond_to?(name)

        raise UndefinedTargetError, "no public method #{name.inspect} on target"
      end

      # Consult the target's opt-in narrowing predicate. A bound object
      # may define a private +respond_to_guest?(name)+ to restrict which of its
      # methods the guest reaches; a falsy answer rejects the dispatch.
      # The predicate composes beneath {#reject_meta_method!} — it only narrows,
      # never re-opening the reflection surface the floor rejects — and is
      # consulted with the private surface included so the guest's +public_send+
      # dispatch can never reach +respond_to_guest?+ itself.
      def reject_unexposed!(target, name)
        return unless target.respond_to?(:respond_to_guest?, true)
        return if target.__send__(:respond_to_guest?, name)

        raise UndefinedTargetError, "method #{name.inspect} is not exposed to the guest"
      end

      # A Kobako::Handle arriving as a positional or keyword
      # argument identifies a host-side object previously allocated by a prior
      # transport call's Handle wrap. Resolve it back to the Ruby object before
      # the dispatch reaches +public_send+.
      def resolve_arg(value, handler)
        value.is_a?(Kobako::Handle) ? require_live_object!(value.id, handler) : value
      end

      # Resolve a Request target to the Ruby object the registry (or
      # Catalog::Handles) holds. String targets go through the registry;
      # Handle targets (ext 0x01) go through the Catalog::Handles.
      #
      # Target type is already validated by +Transport::Request.decode+
      # before this method is reached, so no else-branch is needed here —
      # the wire layer is the system boundary that enforces the invariant.
      def resolve_target(target, namespaces, handler)
        case target
        when String
          resolve_path(target, namespaces)
        when Kobako::Handle
          resolve_handle(target, handler)
        end
      end

      def resolve_path(path, namespaces)
        namespaces.lookup(path)
      rescue KeyError => e
        raise UndefinedTargetError, e.message
      end

      def resolve_handle(handle, handler)
        require_live_object!(handle.id, handler)
      end

      # Resolve +id+ through the Catalog::Handles. An unknown id
      # surfaces as UndefinedTargetError.
      def require_live_object!(id, handler)
        handler.fetch(id)
      rescue Kobako::SandboxError => e
        raise UndefinedTargetError, e.message
      end

      # Encode +value+ as a +Response.ok+ envelope. When the value is not
      # wire-representable per the codec's type mapping, the
      # +UnsupportedType+ rescue routes it through the
      # Catalog::Handles via {#wrap_as_handle} and re-encodes with the Capability
      # Handle in place. The happy path encodes exactly once.
      def encode_ok(value, handler)
        response = Kobako::Transport::Response.ok(value)
        response.encode
      rescue Kobako::Codec::UnsupportedType
        encode_ok(wrap_as_handle(value, handler), handler)
      end

      # Allocate +value+ in the Sandbox's Catalog::Handles and return a +Handle+
      # that the wire codec can carry. Used as the fallback path of
      # {#encode_ok} when +value+ has no wire representation.
      def wrap_as_handle(value, handler)
        handler.alloc(value)
      end

      def encode_error(type, message)
        fault = Kobako::Fault.new(type: type, message: message)
        response = Kobako::Transport::Response.error(fault)
        response.encode
      end
    end
  end
end
