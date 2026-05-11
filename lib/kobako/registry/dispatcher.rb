# frozen_string_literal: true

module Kobako
  class Registry
    # Pure-function dispatcher for guest-initiated RPC calls. Decodes a
    # msgpack-encoded Request envelope, resolves the target object through the
    # Registry (path lookup or HandleTable lookup), invokes the method, and
    # returns a msgpack-encoded Response envelope.
    #
    # The module is stateless — all mutable state is threaded through the
    # +registry+ argument so Dispatcher has no instance variables and no side
    # effects beyond mutating the HandleTable via +alloc+ when a non-wire-
    # representable return value must be wrapped ({SPEC.md §B-14}[link:../../../SPEC.md]).
    #
    # Entry point:
    #
    #   Kobako::Registry::Dispatcher.dispatch(request_bytes, registry)
    #   # => msgpack-encoded Response bytes (never raises)
    module Dispatcher
      module_function

      # Internal sentinel raised when target resolution fails. Mapped to
      # Response.err with type="undefined". Contained at the wire boundary —
      # not part of the public Kobako error taxonomy
      # ({SPEC.md §E-xx}[link:../../../SPEC.md]).
      class UndefinedTargetError < StandardError; end

      # Internal sentinel raised when a Handle target resolves to the
      # +:disconnected+ sentinel in the HandleTable (ABA protection,
      # {SPEC.md §E-14}[link:../../../SPEC.md]). Mapped to Response.err with
      # type="disconnected". Contained at the wire boundary.
      class DisconnectedTargetError < StandardError; end

      # Dispatch a single RPC request and return the encoded response bytes.
      # Called by +Kobako::Registry#dispatch+ which is invoked from the Rust
      # ext inside +__kobako_rpc_call+. +request_bytes+ is the msgpack-encoded
      # Request envelope. +registry+ is the live registry for this run, used
      # to resolve path-based targets via +#lookup+ and to access the
      # +#handle_table+ for Handle-based targets and return-value wrapping.
      # Always returns a binary String — never raises. Any failure during
      # decode, lookup, or method invocation is reified as a Response.err
      # envelope so the guest sees the failure as a normal RPC error rather
      # than a wasm trap
      # ({SPEC.md §Registry 實作要點 §dispatch 流程}[link:../../../SPEC.md]).
      def dispatch(request_bytes, registry)
        encode_ok(wrap_return(perform_dispatch(request_bytes, registry), registry))
      rescue StandardError => e
        encode_dispatch_error(e)
      end

      # Map an error raised during dispatch to a Response.err envelope.
      # +error+ is the +StandardError+ caught at the dispatch boundary. Returns
      # a msgpack-encoded Response envelope (binary). Four error buckets
      # ({SPEC.md §dispatch 流程}[link:../../../SPEC.md]): +Wire::Error+ →
      # type="runtime" (wire decode failed); +DisconnectedTargetError+ →
      # type="disconnected" (E-14); +UndefinedTargetError+ → type="undefined"
      # (E-13); +ArgumentError+ → type="argument" (B-12 arity mismatch);
      # everything else → type="runtime".
      def encode_dispatch_error(error)
        case error
        when Kobako::Wire::Error     then encode_err("runtime", "wire decode failed: #{error.message}")
        when DisconnectedTargetError then encode_err("disconnected", error.message)
        when UndefinedTargetError    then encode_err("undefined", error.message)
        when ArgumentError           then encode_err("argument", error.message)
        else                              encode_err("runtime", "#{error.class}: #{error.message}")
        end
      end

      def perform_dispatch(request_bytes, registry)
        request = Kobako::Wire::Envelope.decode_request(request_bytes)
        handle_table = registry.handle_table
        target_object = resolve_target(request.target, registry, handle_table)
        args = request.args.map { |v| resolve_arg(v, handle_table) }
        kwargs = request.kwargs.transform_values { |v| resolve_arg(v, handle_table) }
        invoke(target_object, request.method_name, args, kwargs)
      end

      # {SPEC.md §B-16}[link:../../../SPEC.md] — A Wire::Handle arriving as a positional or keyword
      # argument identifies a host-side object previously allocated by a prior
      # RPC's Handle wrap (B-14). Resolve it back to the Ruby object before
      # the dispatch reaches +public_send+. A Handle whose entry is the
      # +:disconnected+ sentinel (E-14) raises DisconnectedTargetError so
      # the dispatcher emits a Response.err with type="disconnected".
      def resolve_arg(value, handle_table)
        case value
        when Kobako::Wire::Handle
          fetch_live_object(value.id, handle_table)
        else
          value
        end
      end

      # Resolve a Request target to the Ruby object the Registry (or
      # HandleTable) holds. String targets go through the Registry;
      # Handle targets (ext 0x01) go through the HandleTable.
      #
      # Target type is already validated by +Wire::Envelope.decode_request+
      # before this method is reached, so no else-branch is needed here —
      # the wire layer is the system boundary that enforces the invariant.
      def resolve_target(target, registry, handle_table)
        case target
        when String
          resolve_path(target, registry)
        when Kobako::Wire::Handle
          resolve_handle(target, handle_table)
        end
      end

      def resolve_path(path, registry)
        registry.lookup(path)
      rescue KeyError => e
        raise UndefinedTargetError, e.message
      end

      def resolve_handle(handle, handle_table)
        fetch_live_object(handle.id, handle_table)
      end

      # Resolve +id+ through the HandleTable, distinguishing the
      # +:disconnected+ sentinel (E-14) from an unknown id (E-13).
      def fetch_live_object(id, handle_table)
        object = handle_table.fetch(id)
        raise DisconnectedTargetError, "Handle id #{id} is disconnected" if object == :disconnected

        object
      rescue Kobako::HandleTableError => e
        raise UndefinedTargetError, e.message
      end

      # Invoke +method+ on +target+ with positional args and string-keyed
      # kwargs (symbolized at the boundary per {SPEC.md §B-12 Notes}[link:../../../SPEC.md]).
      #
      # Empty kwargs is wire-uniform ({SPEC.md line 815}[link:../../../SPEC.md]: "empty kwargs is
      # encoded as empty map +0x80+, never absent"). Methods whose signature
      # accepts no keyword arguments must still dispatch when the wire carries
      # an empty kwargs map; the empty-kwargs branch omits the +**+ splat so
      # Ruby 3.x's strict kwargs separation does not reject the call.
      def invoke(target, method, args, kwargs)
        sym_kwargs = symbolize_kwargs(kwargs)
        if sym_kwargs.empty?
          target.public_send(method.to_sym, *args)
        else
          target.public_send(method.to_sym, *args, **sym_kwargs)
        end
      end

      def symbolize_kwargs(kwargs)
        kwargs.each_with_object({}) do |(key, value), acc|
          utf8_key = key.encoding == Encoding::UTF_8 ? key : key.dup.force_encoding(Encoding::UTF_8)
          acc[utf8_key.to_sym] = value
        end
      end

      # {SPEC.md §B-14}[link:../../../SPEC.md] — When a bound Service method returns a value that is not
      # wire-representable per B-13's type mapping, the wire layer routes it
      # through the HandleTable and the guest receives a Capability Handle in
      # place of the raw object.
      def wrap_return(value, registry)
        Kobako::Wire::Encoder.encode(value)
        value
      rescue Kobako::Wire::UnsupportedType
        Kobako::Wire::Handle.new(registry.handle_table.alloc(value))
      end

      def encode_ok(value)
        response = Kobako::Wire::Envelope::Response.ok(value)
        Kobako::Wire::Envelope.encode_response(response)
      end

      def encode_err(type, message)
        exception = Kobako::Wire::Exception.new(type: type, message: message)
        response = Kobako::Wire::Envelope::Response.err(exception)
        Kobako::Wire::Envelope.encode_response(response)
      end
    end
  end
end
