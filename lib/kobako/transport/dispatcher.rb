# frozen_string_literal: true

require_relative "../codec"
require_relative "../payload"
require_relative "call"
require_relative "reflection"
require_relative "yielder"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns the pure-function dispatcher that answers a routed Call.
  module Transport
    # Pure-function dispatcher for guest-initiated Calls. The native side
    # has already decoded the core envelope, so this resolves the target
    # through the per-invocation path +resolver+ (the +Context+, whose
    # +#lookup+ layers per-invocation providers over the static bindings)
    # or Catalog::Handles, decodes only the payload, invokes the method,
    # and answers +[ok, bytes]+ — which the native side puts on the
    # Reply's ok or fault arm. It never raises.
    #
    # The module is stateless — all mutable state is threaded through
    # arguments so Dispatcher has no instance variables and no side
    # effects beyond mutating the Catalog::Handles via +alloc+ when a
    # non-wire-representable return value must be wrapped.
    module Dispatcher
      # Throw tag for the Yielder's break unwind back to the
      # dispatcher's +catch+ frame. +private_constant+ is a
      # convention boundary — not a defence.
      BREAK_THROW = :__kobako_break__
      private_constant :BREAK_THROW

      module_function

      # Internal sentinel raised when target resolution fails. Becomes a
      # Fault with type="undefined". Contained at the wire boundary —
      # not part of the public Kobako error taxonomy.
      class UndefinedTargetError < StandardError; end

      # Answer a single routed Call with +[ok, bytes]+, which the native
      # side puts on the Reply's ok or fault arm. Invoked from the
      # per-invocation dispatch Proc that
      # +Kobako::Context+ hands to +Runtime#eval+ / +#run+; +resolver+,
      # +handler+, and +yield_to_guest+ are captured in that Proc's
      # closure so the Dispatcher stays stateless and neither the resolver
      # nor the Context needs to publish accessors for the per-invocation
      # +Catalog::Handles+ or +Runtime+. +yield_to_guest+ is a +String → String+ callable
      # (the ext's per-dispatch +Kobako::Runtime::GuestYielder+) used only
      # when the Call carries +block_given: true+. Never raises — every
      # failure path takes the fault arm instead, so the guest sees a
      # transport error rather than a wasm trap.
      #
      # The decode runs inside +Codec.track_handles+ so #resolve_call_args
      # can skip the argument walk when no Capability Handle crossed the
      # wire.
      def dispatch(call, resolver, handler, yield_to_guest)
        yielder = Yielder.new(yield_to_guest, BREAK_THROW, handler) if call.block_given
        [true, encode_ok(run(call, resolver, handler, yielder), handler), nil] # : [bool, String, String?]
      # StandardError is the boundary by intent: a Service method's
      # application fault folds into a guest-rescuable fault, while a
      # host-process failure (NoMemoryError, SignalException, a bare Exception)
      # stays uncaught and traps the invocation rather than being masked as a
      # rescuable fault.
      rescue StandardError => e
        [false, *caught_fault(e)] # : [bool, String, String?]
      ensure
        yielder&.invalidate!
      end

      # Decode the payload, resolve the receiver, and run the method inside
      # the +catch+ frame a guest +break+ unwinds to. Split from #dispatch
      # so the reply-shaping and the failure boundary stay one glance wide.
      def run(call, resolver, handler, yielder)
        arguments, carried_handle = Kobako::Codec.track_handles { Payload::Arguments.decode(call.payload) }
        receiver = resolve_target(call.target, resolver, handler)
        args, kwargs = resolve_call_args(arguments, handler, carried_handle)
        catch(BREAK_THROW) { invoke(receiver, call.method_name, args, kwargs, yielder) }
      end

      # Resolve positional and keyword arguments off the decoded payload in
      # one step. +carried_handle+ reports whether the decode carried any
      # Capability Handle; when it did not, every argument resolves to
      # itself, so the decoded values pass straight through and the walk is
      # skipped entirely. Otherwise both go through #resolve_arg so Handles
      # round-trip back to the host-side Ruby object before the call reaches
      # +public_send+.
      def resolve_call_args(arguments, handler, carried_handle)
        return [arguments.args, arguments.kwargs] unless carried_handle

        [arguments.args.map { |v| resolve_arg(v, handler) },
         arguments.kwargs.transform_values { |v| resolve_arg(v, handler) }]
      end

      # Map an error caught at the dispatch boundary to the message and the
      # category the native side frames into the Reply's fault arm. +error+
      # is the +StandardError+ caught by #dispatch's rescue; the category
      # tells the guest which kind of failure it was so it can raise the
      # matching proxy-side error.
      #
      # The class prefix marks a Service's own exception and nothing else:
      # it is the +<class>: <message>+ shape a Host App is told to keep
      # secrets out of, so wearing it says the Service raised. kobako's own
      # refusals answer under their own wording instead of borrowing that
      # shape.
      def caught_fault(error)
        case error
        when Kobako::Codec::Error   then fault("internal",
                                               "Sandbox could not read the request: #{error.message}")
        when HandleExhaustedError   then fault("internal", error.message)
        when UndefinedTargetError   then fault("undefined", error.message)
        when ArgumentError          then fault("argument", error.message)
        when Kobako::SandboxError   then fault("runtime", error.message)
        else                             fault("runtime", "#{error.class}: #{error.message}")
        end
      end

      # Dispatch +method+ on +target+. +kwargs+ is already Symbol-keyed
      # (the +Payload::Arguments+ invariant pins it). The empty-kwargs branch omits
      # the +**+ splat so Ruby 3.x's strict kwargs separation does not
      # reject calls to no-kwarg methods when the wire carries the
      # uniform empty-map shape.
      #
      # +yielder+ is the host-side Yielder materialised when the guest
      # call site supplied a block; its Yielder#to_proc
      # rides the +&block+ slot. +&nil+ is a no-op block argument in Ruby,
      # so the same call site handles both cases without an explicit
      # conditional.
      def invoke(target, method, args, kwargs, yielder = nil)
        name = method.to_sym
        reject_unreachable!(target, name)
        block = yielder&.to_proc
        if kwargs.empty?
          target.public_send(name, *args, &block)
        else
          target.public_send(name, *args, **kwargs, &block)
        end
      end

      # Guard the +public_send+ below: Reflection decides what counts as
      # Service behaviour on this target, and its refusal reason becomes
      # the guest's +undefined+ fault. Both the ambient-surface floor and
      # the target's own narrowing predicate answer through it, so a
      # rejected name discloses nothing about which of the two refused.
      def reject_unreachable!(target, name)
        reason = Reflection.refusal(target, name)
        raise UndefinedTargetError, reason if reason
      end

      # Resolve every Kobako::Handle in an argument — bare or nested in an
      # Array / Hash — back to its host object before the dispatch reaches
      # +public_send+, symmetric with the guest→host return path. A Handle id
      # with no live entry surfaces as an unrecognized target.
      def resolve_arg(value, handler)
        Kobako::Codec::HandleWalk.deep_restore(value, handler)
      rescue Kobako::SandboxError => e
        raise UndefinedTargetError, e.message
      end

      # Resolve a Call target to the Ruby object the path +resolver+ (or
      # Catalog::Handles) holds. The native side already discriminated the
      # two forms off the core envelope's +kind+ tag: a String is a bound
      # constant's path, an Integer is a Capability Handle id. No
      # else-branch is needed — the envelope layer is the system boundary
      # that enforces the invariant.
      def resolve_target(target, resolver, handler)
        case target
        when String
          resolve_path(target, resolver)
        when Integer
          require_live_object!(target, handler)
        end
      end

      def resolve_path(path, resolver)
        resolver.lookup(path)
      rescue KeyError => e
        raise UndefinedTargetError, e.message
      end

      # Resolve +id+ through the Catalog::Handles. An unknown id
      # surfaces as UndefinedTargetError.
      def require_live_object!(id, handler)
        handler.fetch(id)
      rescue Kobako::SandboxError => e
        raise UndefinedTargetError, e.message
      end

      # Encode +value+ as the body of a Reply's ok arm — the value alone,
      # since the envelope's tag already carries the success. A value that
      # is not wire-representable per the codec's type mapping raises
      # +UnsupportedTypeError+; the rescue routes it through the
      # Catalog::Handles via #wrap_as_handle and re-encodes with the
      # Capability Handle in place. The happy path encodes exactly once.
      #
      # Any other codec fault here is the answer failing to encode, not the
      # request failing to decode, so it is restated before the boundary
      # sees it — the two are otherwise the same class and would report
      # under the same wording.
      def encode_ok(value, handler)
        Kobako::Codec::Encoder.encode(value)
      rescue Kobako::Codec::UnsupportedTypeError
        encode_ok(wrap_as_handle(value, handler), handler)
      rescue Kobako::Codec::Error => e
        raise Kobako::SandboxError, "Sandbox could not write the Service's answer: #{e.message}"
      end

      # Allocate +value+ in the Sandbox's Catalog::Handles and return a +Handle+
      # that the wire codec can carry. Used as the fallback path of
      # #encode_ok when +value+ has no wire representation.
      def wrap_as_handle(value, handler)
        handler.alloc(value)
      end

      # +message+ folds to UTF-8 first: Ruby core builds some exception
      # messages as ASCII-8BIT (the arity ArgumentError, for one), and
      # the envelope requires UTF-8 of the text fields it frames.
      def fault(type, message)
        [message.encode(Encoding::UTF_8, invalid: :replace, undef: :replace), type] # : [String, String]
      end
    end
  end
end
