# frozen_string_literal: true

module Kobako
  module RPC
    class Server
      # Pure-function dispatcher for guest-initiated RPC calls. Decodes a
      # msgpack-encoded Request envelope, resolves the target object through the
      # Server (path lookup or HandleTable lookup), invokes the method, and
      # returns a msgpack-encoded Response envelope.
      #
      # The module is stateless — all mutable state is threaded through the
      # +server+ argument so Dispatcher has no instance variables and no side
      # effects beyond mutating the HandleTable via +alloc+ when a non-wire-
      # representable return value must be wrapped ({SPEC.md B-14}[link:../../../../SPEC.md]).
      #
      # Entry point:
      #
      #   Kobako::RPC::Server::Dispatcher.dispatch(request_bytes, server)
      #   # => msgpack-encoded Response bytes (never raises)
      module Dispatcher
        module_function

        # Internal sentinel raised when target resolution fails. Mapped to
        # Response.err with type="undefined". Contained at the wire boundary —
        # not part of the public Kobako error taxonomy
        # ({SPEC.md E-xx}[link:../../../../SPEC.md]).
        class UndefinedTargetError < StandardError; end

        # Internal sentinel raised when a Handle target resolves to the
        # +:disconnected+ sentinel in the HandleTable (ABA protection,
        # {SPEC.md E-14}[link:../../../../SPEC.md]). Mapped to Response.err with
        # type="disconnected". Contained at the wire boundary.
        class DisconnectedTargetError < StandardError; end

        # Dispatch a single RPC request and return the encoded response bytes.
        # Called by +Kobako::RPC::Server#dispatch+ which is invoked from the
        # Rust ext inside +__kobako_dispatch+. +request_bytes+ is the
        # msgpack-encoded Request envelope. +server+ is the live Server for
        # this run, used to resolve path-based targets via +#lookup+ and to
        # access the +#handle_table+ for Handle-based targets and return-value
        # wrapping. Always returns a binary String — never raises. Any failure
        # during decode, lookup, or method invocation is reified as a
        # Response.err envelope so the guest sees the failure as a normal RPC
        # error rather than a wasm trap
        # ({SPEC.md B-12}[link:../../../../SPEC.md]).
        def dispatch(request_bytes, server)
          value = perform_dispatch(request_bytes, server)
          encode_ok_or_wrap(value, server)
        rescue StandardError => e
          encode_dispatch_error(e)
        end

        # Map an error raised during dispatch to a Response.err envelope.
        # +error+ is the +StandardError+ caught at the dispatch boundary. Returns
        # a msgpack-encoded Response envelope (binary). Four error buckets
        # ({SPEC.md B-12}[link:../../../../SPEC.md]): +Kobako::Codec::Error+ →
        # type="runtime" (wire decode failed); +DisconnectedTargetError+ →
        # type="disconnected" (E-14); +UndefinedTargetError+ → type="undefined"
        # (E-13); +ArgumentError+ → type="argument" (B-12 arity mismatch);
        # everything else → type="runtime".
        def encode_dispatch_error(error)
          case error
          when Kobako::Codec::Error then encode_err("runtime", "wire decode failed: #{error.message}")
          when DisconnectedTargetError then encode_err("disconnected", error.message)
          when UndefinedTargetError    then encode_err("undefined", error.message)
          when ArgumentError           then encode_err("argument", error.message)
          else                              encode_err("runtime", "#{error.class}: #{error.message}")
          end
        end

        def perform_dispatch(request_bytes, server)
          request = Kobako::Wire::Envelope.decode_request(request_bytes)
          handle_table = server.handle_table
          target_object = resolve_target(request.target, server, handle_table)
          args = request.args.map { |v| resolve_arg(v, handle_table) }
          kwargs = request.kwargs.transform_values { |v| resolve_arg(v, handle_table) }
          invoke(target_object, request.method_name, args, kwargs)
        end

        # Dispatch +method+ on +target+. +kwargs+ is already Symbol-keyed
        # (the +Envelope::Request+ invariant pins it). The empty-kwargs
        # branch omits the +**+ splat so Ruby 3.x's strict kwargs
        # separation does not reject calls to no-kwarg methods when the
        # wire carries the uniform empty-map shape.
        def invoke(target, method, args, kwargs)
          if kwargs.empty?
            target.public_send(method.to_sym, *args)
          else
            target.public_send(method.to_sym, *args, **kwargs)
          end
        end

        # {SPEC.md B-16}[link:../../../../SPEC.md] — A Wire::Handle arriving as a positional or keyword
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

        # Resolve a Request target to the Ruby object the Server (or
        # HandleTable) holds. String targets go through the Server;
        # Handle targets (ext 0x01) go through the HandleTable.
        #
        # Target type is already validated by +Wire::Envelope.decode_request+
        # before this method is reached, so no else-branch is needed here —
        # the wire layer is the system boundary that enforces the invariant.
        def resolve_target(target, server, handle_table)
          case target
          when String
            resolve_path(target, server)
          when Kobako::Wire::Handle
            resolve_handle(target, handle_table)
          end
        end

        def resolve_path(path, server)
          server.lookup(path)
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

        # Encode +value+ as a +Response.ok+ envelope. When the value is not
        # wire-representable per {SPEC.md B-13}[link:../../../../SPEC.md]'s type
        # mapping, the +UnsupportedType+ rescue routes it through the
        # HandleTable and re-encodes with the Capability Handle in place
        # ({SPEC.md B-14}[link:../../../../SPEC.md]). The happy path encodes
        # exactly once.
        def encode_ok_or_wrap(value, server)
          encode_ok(value)
        rescue Kobako::Codec::UnsupportedType
          encode_ok(Kobako::Wire::Handle.new(server.handle_table.alloc(value)))
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
end
