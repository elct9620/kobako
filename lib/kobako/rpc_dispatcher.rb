# frozen_string_literal: true

require_relative "wire/encoder"
require_relative "wire/envelope"
require_relative "wire/exception"
require_relative "wire/error"
require_relative "wire/handle"

module Kobako
  # Kobako::RpcDispatcher — host-side RPC dispatch entry point invoked by
  # the Rust ext from inside `__kobako_rpc_call`.
  #
  # SPEC.md → Behavior B-12 (target string `"Group::Member"` dispatch),
  # B-13 (positional + kwargs argument unwrap), B-14 (stateful return
  # value → Capability Handle), B-16 (Handle as argument), B-17 (Handle
  # as target). The Rust ext reads the request bytes from guest linear
  # memory, hands them to {#call}, then writes the returned response
  # bytes back into guest memory.
  #
  # The dispatcher owns four concerns:
  #
  #   1. Decoding the Request envelope (Kobako::Wire::Envelope.decode_request).
  #   2. Resolving the target — Registry path (B-12) or HandleTable
  #      lookup (B-17) — and resolving any Wire::Handle args/kwargs to
  #      their bound objects via the HandleTable (B-16).
  #   3. Invoking the bound Ruby object via `public_send` with positional
  #      args + symbolized kwargs.
  #   4. Encoding the result as a Response.ok or Response.err. Non-wire-
  #      representable return values are routed through the HandleTable
  #      and surface to the guest as a Capability Handle (B-14). Ruby
  #      exceptions map to the wire Exception envelope per E-11/E-12/E-15.
  #
  # The byte-in / byte-out signature keeps the Rust↔Ruby boundary small:
  # the Rust import callback only needs to ferry a single pair of byte
  # strings, never a structured value.
  class RpcDispatcher
    # Build a dispatcher bound to a Service Registry and a HandleTable.
    #
    # @param registry [Kobako::Service::Registry]
    # @param handle_table [Kobako::HandleTable]
    def initialize(registry:, handle_table:)
      @registry = registry
      @handle_table = handle_table
    end

    # Dispatch a single RPC request and return the encoded response bytes.
    #
    # Always returns a binary String — never raises. Any failure during
    # decode, lookup, or method invocation is reified as a Response.err
    # envelope so the guest sees the failure as a normal RPC error rather
    # than a wasm trap.
    #
    # @param request_bytes [String] msgpack-encoded Request envelope.
    # @return [String] msgpack-encoded Response envelope (binary).
    def call(request_bytes)
      encode_ok(wrap_return(dispatch(request_bytes)))
    rescue Kobako::Wire::Error => e
      encode_err("runtime", "wire decode failed: #{e.message}")
    rescue UndefinedTargetError => e
      encode_err("undefined", e.message)
    rescue ArgumentError => e
      encode_err("argument", e.message)
    rescue StandardError => e
      encode_err("runtime", "#{e.class}: #{e.message}")
    end

    private

    def dispatch(request_bytes)
      request = Kobako::Wire::Envelope.decode_request(request_bytes)
      target_object = resolve_target(request.target)
      args = request.args.map { |v| resolve_arg(v) }
      kwargs = request.kwargs.transform_values { |v| resolve_arg(v) }
      invoke(target_object, request.method_name, args, kwargs)
    end

    # SPEC B-16 — A Wire::Handle that arrives as a positional or keyword
    # argument identifies a host-side object previously allocated by a
    # prior RPC's Handle wrap (B-14). Resolve it back to the Ruby object
    # before the dispatch reaches `public_send`, so the Service method
    # receives the actual object, not a wire-layer proxy.
    def resolve_arg(value)
      case value
      when Kobako::Wire::Handle
        @handle_table.fetch(value.id)
      else
        value
      end
    rescue Kobako::HandleTableError => e
      raise UndefinedTargetError, e.message
    end

    # Resolve a Request target to the Ruby object the Service Registry (or
    # HandleTable) holds. String targets like "Group::Member" go through
    # the Registry; Handle targets (ext 0x01) go through the HandleTable.
    def resolve_target(target)
      case target
      when String
        resolve_path(target)
      when Kobako::Wire::Handle
        resolve_handle(target)
      else
        raise UndefinedTargetError, "unsupported target type #{target.class}"
      end
    end

    def resolve_path(path)
      @registry.lookup(path)
    rescue KeyError => e
      raise UndefinedTargetError, e.message
    end

    def resolve_handle(handle)
      @handle_table.fetch(handle.id)
    rescue Kobako::HandleTableError => e
      raise UndefinedTargetError, e.message
    end

    # Invoke +method+ on +target+ with positional args and string-keyed
    # kwargs (symbolized at the boundary per SPEC B-12 Notes).
    def invoke(target, method, args, kwargs)
      sym_kwargs = kwargs.transform_keys(&:to_sym)
      if sym_kwargs.empty?
        target.public_send(method.to_sym, *args)
      else
        target.public_send(method.to_sym, *args, **sym_kwargs)
      end
    end

    # SPEC B-14 — When a bound Service method returns a value that is not
    # wire-representable per B-13's type mapping, the wire layer routes it
    # through the HandleTable and the guest receives a Capability Handle
    # in place of the raw object.
    #
    # Policy: probe the return value against the codec's encodability
    # check; the codec raises {Kobako::Wire::UnsupportedType} for any
    # value outside the closed type set (nil/bool/int/float/str/bin/
    # array/map of those, plus Wire::Handle / Wire::Exception). Any such
    # raise is the trigger to wrap. Wire::Handle returned directly by a
    # Service method (rare — Host App has no public allocation API) is
    # left as-is so an explicitly handed-back Handle round-trips.
    def wrap_return(value)
      Kobako::Wire::Encoder.encode(value)
      value
    rescue Kobako::Wire::UnsupportedType
      Kobako::Wire::Handle.new(@handle_table.alloc(value))
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

    # Internal sentinel — raised when target resolution fails. Mapped to
    # Response.err with type="undefined" by {#call}. Not part of the
    # public Kobako error taxonomy because the failure is contained at
    # the wire boundary and never reaches the Host App.
    class UndefinedTargetError < StandardError; end
  end
end
