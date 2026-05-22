# frozen_string_literal: true

require_relative "../codec"
require_relative "../yield"

module Kobako
  module RPC
    # Pure-function dispatcher for guest-initiated RPC calls. Decodes a
    # msgpack-encoded Request envelope, resolves the target object through
    # the Server (path lookup or HandleTable lookup), invokes the method,
    # and returns a msgpack-encoded Response envelope.
    #
    # The module is stateless — all mutable state is threaded through the
    # +server+ argument so Dispatcher has no instance variables and no side
    # effects beyond mutating the HandleTable via +alloc+ when a non-wire-
    # representable return value must be wrapped ({docs/behavior.md B-14}[link:../../../docs/behavior.md]).
    #
    # Entry point:
    #
    #   Kobako::RPC::Dispatcher.dispatch(request_bytes, server)
    #   # => msgpack-encoded Response bytes (never raises)
    module Dispatcher
      module_function

      # Internal sentinel raised when target resolution fails. Mapped to
      # Response.error with type="undefined". Contained at the wire boundary —
      # not part of the public Kobako error taxonomy
      # ({docs/behavior.md E-xx}[link:../../../docs/behavior.md]).
      class UndefinedTargetError < StandardError; end

      # Internal sentinel raised when a Handle target resolves to the
      # +:disconnected+ sentinel in the HandleTable (ABA protection,
      # {docs/behavior.md E-14}[link:../../../docs/behavior.md]). Mapped to Response.error with
      # type="disconnected". Contained at the wire boundary.
      class DisconnectedTargetError < StandardError; end

      # Dispatch a single RPC request and return the encoded response bytes.
      # Called by +Kobako::RPC::Channel#dispatch+ which is invoked from
      # the Rust ext inside +__kobako_dispatch+. +request_bytes+ is the
      # msgpack-encoded Request envelope. +server+ resolves path-based
      # Member targets via +#lookup+. +handle_table+ is the Sandbox's
      # HandleTable, injected separately so Dispatcher does not depend
      # on Server publishing a Handle accessor — Handle is a
      # Sandbox-level domain entity (B-19) and the dispatcher is its
      # only consumer here. +channel+ is the +Kobako::RPC::Channel+ the
      # block proxy uses to re-enter the guest (B-24); also injected
      # rather than read off Server so the namespace registry stays
      # Channel-unaware. Always returns a binary String — never raises.
      # Any failure during decode, lookup, or method invocation is
      # reified as a Response.error envelope so the guest sees the
      # failure as a normal RPC error rather than a wasm trap
      # ({docs/behavior.md B-12}[link:../../../docs/behavior.md]).
      def dispatch(request_bytes, server, handle_table, channel)
        request = Kobako::RPC.decode_request(request_bytes)
        target = resolve_target(request.target, server, handle_table)
        args, kwargs = resolve_call_args(request, handle_table)
        block_proxy = build_block_proxy(channel) if request.block_given
        value = invoke(target, request.method_name, args, kwargs, block_proxy)
        encode_ok(value, handle_table)
      rescue StandardError => e
        encode_caught_error(e)
      end

      # Resolve positional and keyword arguments off +request+ in one
      # step. Both pass through {#resolve_arg} so Capability Handles
      # round-trip back to the host-side Ruby object before the call
      # reaches +public_send+.
      def resolve_call_args(request, handle_table)
        args = request.args.map { |v| resolve_arg(v, handle_table) }
        kwargs = request.kwargs.transform_values { |v| resolve_arg(v, handle_table) }
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

      # Build the host-side yield proxy passed to the Service method as
      # its +&block+ argument ({docs/behavior.md B-23 /
      # B-24}[link:../../../docs/behavior.md]). The proxy is a +Proc+
      # (not a +Lambda+) so it inherits the loose arity Ruby's +&block+
      # convention implies. Each invocation serialises positional args
      # as a msgpack array, hands the bytes to the Channel for guest
      # re-entry, and classifies the +YieldResponse+:
      #
      #   * +tag 0x01+ ok    — return the decoded value to +yield+'s caller
      #   * +tag 0x02+ break — raise (S6b wires the +catch+/+throw+ path
      #     that unwinds the Service method per B-25; for now break
      #     surfaces as a controlled error)
      #   * +tag 0x04+ error — raise with the +{class, message,
      #     backtrace}+ payload the guest produced
      def build_block_proxy(channel)
        proc do |*args|
          response = Kobako::Yield.decode_response(channel.yield_to_block(Kobako::Codec::Encoder.encode(args)))
          next response.value if response.ok?

          raise yield_failure(response.value, default: response.break? ? "break" : "yield error")
        end
      end

      # Reify a +YieldResponse+ error / break payload into a
      # +RuntimeError+ the Service method observes at its +yield+ call
      # site. The +{class, message, backtrace}+ shape mirrors the
      # +Kobako::Yield::Response+ tag 0x04 payload; +default+ provides a
      # fallback when the payload is not a Hash (defensive — the
      # guest's encoder always emits the map shape).
      def yield_failure(payload, default:)
        return RuntimeError.new(default) unless payload.is_a?(Hash)

        klass = payload["class"] || "RuntimeError"
        message = payload["message"] || default
        RuntimeError.new("#{klass}: #{message}")
      end

      # {docs/behavior.md B-16}[link:../../../docs/behavior.md] — An Kobako::Handle arriving as a positional or keyword
      # argument identifies a host-side object previously allocated by a prior
      # RPC's Handle wrap (B-14). Resolve it back to the Ruby object before
      # the dispatch reaches +public_send+. A Handle whose entry is the
      # +:disconnected+ sentinel (E-14) raises DisconnectedTargetError so
      # the dispatcher emits a Response.error with type="disconnected".
      def resolve_arg(value, handle_table)
        case value
        when Kobako::Handle
          require_live_object!(value.id, handle_table)
        else
          value
        end
      end

      # Resolve a Request target to the Ruby object the Server (or
      # HandleTable) holds. String targets go through the Server;
      # Handle targets (ext 0x01) go through the HandleTable.
      #
      # Target type is already validated by +RPC.decode_request+
      # before this method is reached, so no else-branch is needed here —
      # the wire layer is the system boundary that enforces the invariant.
      def resolve_target(target, server, handle_table)
        case target
        when String
          resolve_path(target, server)
        when Kobako::Handle
          resolve_handle(target, handle_table)
        end
      end

      def resolve_path(path, server)
        server.lookup(path)
      rescue KeyError => e
        raise UndefinedTargetError, e.message
      end

      def resolve_handle(handle, handle_table)
        require_live_object!(handle.id, handle_table)
      end

      # Resolve +id+ through the HandleTable, distinguishing the
      # +:disconnected+ sentinel (E-14) from an unknown id (E-13).
      def require_live_object!(id, handle_table)
        object = handle_table.fetch(id)
        raise DisconnectedTargetError, "Handle id #{id} is disconnected" if object == :disconnected

        object
      rescue Kobako::HandleTableError => e
        raise UndefinedTargetError, e.message
      end

      # Encode +value+ as a +Response.ok+ envelope. When the value is not
      # wire-representable per {docs/behavior.md B-13}[link:../../../docs/behavior.md]'s type
      # mapping, the +UnsupportedType+ rescue routes it through the
      # HandleTable via {#wrap_as_handle} and re-encodes with the Capability
      # Handle in place ({docs/behavior.md B-14}[link:../../../docs/behavior.md]). The happy
      # path encodes exactly once.
      def encode_ok(value, handle_table)
        response = Kobako::RPC::Response.ok(value)
        Kobako::RPC.encode_response(response)
      rescue Kobako::Codec::UnsupportedType
        encode_ok(wrap_as_handle(value, handle_table), handle_table)
      end

      # Allocate +value+ in the Sandbox's HandleTable and return a +Handle+
      # that the wire codec can carry ({docs/behavior.md B-14}[link:../../../docs/behavior.md]).
      # Used as the fallback path of {#encode_ok} when +value+ has no wire
      # representation.
      def wrap_as_handle(value, handle_table)
        handle_table.alloc(value)
      end

      def encode_error(type, message)
        fault = Kobako::RPC::Fault.new(type: type, message: message)
        response = Kobako::RPC::Response.error(fault)
        Kobako::RPC.encode_response(response)
      end
    end
  end
end
