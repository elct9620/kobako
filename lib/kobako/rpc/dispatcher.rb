# frozen_string_literal: true

require_relative "../codec"
require_relative "../yield"
require_relative "block_proxy"

module Kobako
  module RPC
    # Pure-function dispatcher for guest-initiated RPC calls. Decodes a
    # msgpack-encoded Request envelope, resolves the target object through
    # the Server (path lookup or Catalog::Handler lookup), invokes the method,
    # and returns a msgpack-encoded Response envelope.
    #
    # The module is stateless — all mutable state is threaded through the
    # +server+ argument so Dispatcher has no instance variables and no side
    # effects beyond mutating the Catalog::Handler via +alloc+ when a non-wire-
    # representable return value must be wrapped ({docs/behavior.md B-14}[link:../../../docs/behavior.md]).
    #
    # Entry point:
    #
    #   Kobako::RPC::Dispatcher.dispatch(request_bytes, server)
    #   # => msgpack-encoded Response bytes (never raises)
    module Dispatcher
      # Throw tag for {#build_block_proxy}'s break unwind back to the
      # dispatcher's +catch+ frame (B-25). +private_constant+ is a
      # convention boundary — not a defence (BLOCK_RESEARCH F-06).
      BREAK_THROW = :__kobako_break__
      private_constant :BREAK_THROW

      module_function

      # Internal sentinel raised when target resolution fails. Mapped to
      # Response.error with type="undefined". Contained at the wire boundary —
      # not part of the public Kobako error taxonomy
      # ({docs/behavior.md E-xx}[link:../../../docs/behavior.md]).
      class UndefinedTargetError < StandardError; end

      # Internal sentinel raised when a Handle target resolves to the
      # +:disconnected+ sentinel in the Catalog::Handler (ABA protection,
      # {docs/behavior.md E-14}[link:../../../docs/behavior.md]). Mapped to Response.error with
      # type="disconnected". Contained at the wire boundary.
      class DisconnectedTargetError < StandardError; end

      # Dispatch a single RPC request and return the encoded Response
      # bytes ({docs/behavior.md B-12}[link:../../../docs/behavior.md]).
      # Called by +Kobako::RPC::Channel#dispatch+ from inside ext's
      # +__kobako_dispatch+ callback. +server+ + +handler+ +
      # +channel+ are injected by the Channel so the Dispatcher stays
      # stateless and Server doesn't need to publish accessors for the
      # Sandbox-owned Catalog::Handler or Instance. Always returns a binary
      # String — every failure path is reified as a Response.error
      # envelope so the guest sees an RPC error rather than a wasm trap.
      def dispatch(request_bytes, server, handler, channel)
        request = Kobako::RPC.decode_request(request_bytes)
        target = resolve_target(request.target, server, handler)
        args, kwargs = resolve_call_args(request, handler)
        block_proxy, invalidator = build_block_proxy(channel) if request.block_given
        value = catch(BREAK_THROW) { invoke(target, request.method_name, args, kwargs, block_proxy) }
        encode_ok(value, handler)
      rescue StandardError => e
        encode_caught_error(e)
      ensure
        invalidator&.call
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
      # rescue. Returns a msgpack-encoded Response envelope (binary). Four
      # error buckets ({docs/behavior.md B-12}[link:../../../docs/behavior.md]):
      # +Kobako::Codec::Error+ → type="runtime" (malformed RPC request);
      # +DisconnectedTargetError+ → type="disconnected" (E-14);
      # +UndefinedTargetError+ → type="undefined" (E-13); +ArgumentError+ →
      # type="argument" (B-12 arity mismatch); everything else →
      # type="runtime".
      def encode_caught_error(error)
        case error
        when Kobako::Codec::Error then encode_error("runtime",
                                                    "Sandbox received a malformed RPC request: #{error.message}")
        when DisconnectedTargetError then encode_error("disconnected", error.message)
        when UndefinedTargetError    then encode_error("undefined", error.message)
        when ArgumentError           then encode_error("argument", error.message)
        else                              encode_error("runtime", "#{error.class}: #{error.message}")
        end
      end

      # Dispatch +method+ on +target+. +kwargs+ is already Symbol-keyed
      # (the +Envelope::Request+ invariant pins it). The empty-kwargs
      # branch omits the +**+ splat so Ruby 3.x's strict kwargs
      # separation does not reject calls to no-kwarg methods when the
      # wire carries the uniform empty-map shape.
      #
      # +block_proxy+ is the host-side yield proxy materialised when
      # the guest call site supplied a block ({docs/behavior.md
      # B-23}[link:../../../docs/behavior.md]). +&nil+ is a no-op block
      # argument in Ruby, so the same call site handles both cases
      # without an explicit conditional.
      def invoke(target, method, args, kwargs, block_proxy = nil)
        if kwargs.empty?
          target.public_send(method.to_sym, *args, &block_proxy)
        else
          target.public_send(method.to_sym, *args, **kwargs, &block_proxy)
        end
      end

      # Delegate to {Kobako::RPC::BlockProxy} so the yield-specific
      # logic (frame_active escape detection, +YieldResponse+ tag
      # routing) lives in its own module. Returns the +[proxy,
      # invalidator]+ pair the Dispatcher hands to the Service method
      # via +&block+ and the +ensure+ block respectively.
      def build_block_proxy(channel)
        BlockProxy.build(channel, BREAK_THROW)
      end

      # {docs/behavior.md B-16}[link:../../../docs/behavior.md] — An Kobako::Handle arriving as a positional or keyword
      # argument identifies a host-side object previously allocated by a prior
      # RPC's Handle wrap (B-14). Resolve it back to the Ruby object before
      # the dispatch reaches +public_send+. A Handle whose entry is the
      # +:disconnected+ sentinel (E-14) raises DisconnectedTargetError so
      # the dispatcher emits a Response.error with type="disconnected".
      def resolve_arg(value, handler)
        case value
        when Kobako::Handle
          require_live_object!(value.id, handler)
        else
          value
        end
      end

      # Resolve a Request target to the Ruby object the Server (or
      # Catalog::Handler) holds. String targets go through the Server;
      # Handle targets (ext 0x01) go through the Catalog::Handler.
      #
      # Target type is already validated by +RPC.decode_request+
      # before this method is reached, so no else-branch is needed here —
      # the wire layer is the system boundary that enforces the invariant.
      def resolve_target(target, server, handler)
        case target
        when String
          resolve_path(target, server)
        when Kobako::Handle
          resolve_handle(target, handler)
        end
      end

      def resolve_path(path, server)
        server.lookup(path)
      rescue KeyError => e
        raise UndefinedTargetError, e.message
      end

      def resolve_handle(handle, handler)
        require_live_object!(handle.id, handler)
      end

      # Resolve +id+ through the Catalog::Handler, distinguishing the
      # +:disconnected+ sentinel (E-14) from an unknown id (E-13).
      def require_live_object!(id, handler)
        object = handler.fetch(id)
        raise DisconnectedTargetError, "Handle id #{id} is disconnected" if object == :disconnected

        object
      rescue Kobako::SandboxError => e
        raise UndefinedTargetError, e.message
      end

      # Encode +value+ as a +Response.ok+ envelope. When the value is not
      # wire-representable per {docs/behavior.md B-13}[link:../../../docs/behavior.md]'s type
      # mapping, the +UnsupportedType+ rescue routes it through the
      # Catalog::Handler via {#wrap_as_handle} and re-encodes with the Capability
      # Handle in place ({docs/behavior.md B-14}[link:../../../docs/behavior.md]). The happy
      # path encodes exactly once.
      def encode_ok(value, handler)
        response = Kobako::RPC::Response.ok(value)
        Kobako::RPC.encode_response(response)
      rescue Kobako::Codec::UnsupportedType
        encode_ok(wrap_as_handle(value, handler), handler)
      end

      # Allocate +value+ in the Sandbox's Catalog::Handler and return a +Handle+
      # that the wire codec can carry ({docs/behavior.md B-14}[link:../../../docs/behavior.md]).
      # Used as the fallback path of {#encode_ok} when +value+ has no wire
      # representation.
      def wrap_as_handle(value, handler)
        handler.alloc(value)
      end

      def encode_error(type, message)
        fault = Kobako::RPC::Fault.new(type: type, message: message)
        response = Kobako::RPC::Response.error(fault)
        Kobako::RPC.encode_response(response)
      end
    end
  end
end
