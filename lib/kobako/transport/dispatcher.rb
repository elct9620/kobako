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
    # non-wire-representable return value must be wrapped
    # ({docs/behavior.md B-14}[link:../../../docs/behavior.md]).
    #
    # Entry point:
    #
    #   Kobako::Transport::Dispatcher.dispatch(request_bytes, namespaces, handler, yield_to_guest)
    #   # => msgpack-encoded Response bytes (never raises)
    module Dispatcher
      # Throw tag for the {Yielder}'s break unwind back to the
      # dispatcher's +catch+ frame (B-25). +private_constant+ is a
      # convention boundary — not a defence.
      BREAK_THROW = :__kobako_break__
      private_constant :BREAK_THROW

      module_function

      # Internal sentinel raised when target resolution fails. Mapped to
      # Response.error with type="undefined". Contained at the wire boundary —
      # not part of the public Kobako error taxonomy
      # ({docs/behavior.md E-12}[link:../../../docs/behavior.md]).
      class UndefinedTargetError < StandardError; end

      # Dispatch a single transport request and return the encoded
      # Response bytes ({docs/behavior.md B-12}[link:../../../docs/behavior.md]).
      # Invoked from the +Runtime#on_dispatch+ Proc that
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
        yielder = Yielder.new(yield_to_guest, BREAK_THROW) if request.block_given
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
        args = request.args.map { |v| resolve_arg(v, handler) }
        kwargs = request.kwargs.transform_values { |v| resolve_arg(v, handler) }
        [args, kwargs]
      end

      # Map an error caught at the dispatch boundary to a +Response.error+
      # envelope. +error+ is the +StandardError+ caught by {#dispatch}'s
      # rescue. Returns a msgpack-encoded Response envelope (binary). Three
      # error buckets ({docs/behavior.md B-12}[link:../../../docs/behavior.md]):
      # +Kobako::Codec::Error+ → type="runtime" (malformed transport request);
      # +UndefinedTargetError+ → type="undefined" (E-13); +ArgumentError+ →
      # type="argument" (B-12 arity mismatch); everything else →
      # type="runtime".
      def encode_caught_error(error)
        case error
        when Kobako::Codec::Error then encode_error("runtime",
                                                    "Sandbox received a malformed transport request: #{error.message}")
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
      # call site supplied a block ({docs/behavior.md
      # B-23}[link:../../../docs/behavior.md]); its {Yielder#to_proc}
      # rides the +&block+ slot. +&nil+ is a no-op block argument in Ruby,
      # so the same call site handles both cases without an explicit
      # conditional.
      def invoke(target, method, args, kwargs, yielder = nil)
        block = yielder&.to_proc
        if kwargs.empty?
          target.public_send(method.to_sym, *args, &block)
        else
          target.public_send(method.to_sym, *args, **kwargs, &block)
        end
      end

      # {docs/behavior.md B-16}[link:../../../docs/behavior.md] — An Kobako::Handle arriving as a positional or keyword
      # argument identifies a host-side object previously allocated by a prior
      # transport call's Handle wrap (B-14). Resolve it back to the Ruby object before
      # the dispatch reaches +public_send+.
      def resolve_arg(value, handler)
        case value
        when Kobako::Handle
          require_live_object!(value.id, handler)
        else
          value
        end
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

      # Resolve +id+ through the Catalog::Handles. An unknown id (E-13)
      # surfaces as UndefinedTargetError.
      def require_live_object!(id, handler)
        handler.fetch(id)
      rescue Kobako::SandboxError => e
        raise UndefinedTargetError, e.message
      end

      # Encode +value+ as a +Response.ok+ envelope. When the value is not
      # wire-representable per {docs/behavior.md B-13}[link:../../../docs/behavior.md]'s type
      # mapping, the +UnsupportedType+ rescue routes it through the
      # Catalog::Handles via {#wrap_as_handle} and re-encodes with the Capability
      # Handle in place ({docs/behavior.md B-14}[link:../../../docs/behavior.md]). The happy
      # path encodes exactly once.
      def encode_ok(value, handler)
        response = Kobako::Transport::Response.ok(value)
        response.encode
      rescue Kobako::Codec::UnsupportedType
        encode_ok(wrap_as_handle(value, handler), handler)
      end

      # Allocate +value+ in the Sandbox's Catalog::Handles and return a +Handle+
      # that the wire codec can carry ({docs/behavior.md B-14}[link:../../../docs/behavior.md]).
      # Used as the fallback path of {#encode_ok} when +value+ has no wire
      # representation.
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
