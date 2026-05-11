# frozen_string_literal: true

require "msgpack"
require_relative "errors"
require_relative "wire/encoder"
require_relative "wire/envelope"
require_relative "wire/exception"
require_relative "wire/error"
require_relative "wire/handle"
require_relative "registry/service_group"
require_relative "registry/handle_table"

module Kobako
  # Kobako::Registry — per-Sandbox container of Service Groups and Handle
  # state. Manages capability injection and guest-initiated RPC dispatch
  # (SPEC.md §Implementation Standards §Architecture).
  #
  # Public API:
  #
  #   registry = Kobako::Registry.new
  #   group = registry.define(:MyService)    # => ServiceGroup
  #   group.bind(:KV, kv_object)             # => group (chainable)
  #   registry.to_preamble                   # => array for Frame 1
  #   registry.dispatch(request_bytes)       # => msgpack bytes
  #
  # Service Groups are defined in +Kobako::Registry::ServiceGroup+
  # (lib/kobako/registry/service_group.rb). The opaque Handle allocator lives
  # in +Kobako::Registry::HandleTable+ (lib/kobako/registry/handle_table.rb).
  class Registry
    # Ruby constant-name pattern (SPEC.md §B-07/B-08 Notes).
    NAME_PATTERN = /\A[A-Z]\w*\z/

    def initialize
      @groups = {}
      @handle_table = HandleTable.new
      @sealed = false
    end

    # Declare or retrieve the Group named +name+ (idempotent — SPEC.md B-10).
    #
    # @param name [Symbol, String] constant-form group name.
    # @return [Kobako::Registry::ServiceGroup] the Group instance.
    # @raise [ArgumentError] when +name+ is malformed, or when called after
    #   the owning Sandbox has been sealed by `#run`.
    def define(name)
      raise ArgumentError, "cannot define after Sandbox#run has been invoked" if @sealed

      name_str = name.to_s
      unless NAME_PATTERN.match?(name_str)
        raise ArgumentError,
              "GroupName must match #{NAME_PATTERN.inspect} (got #{name.inspect})"
      end

      @groups[name_str] ||= ServiceGroup.new(name_str)
    end

    # Resolve a `"GroupName::MemberName"` path to the bound Host object.
    #
    # @param target [String] two-level path with `::` separator.
    # @return [Object] the bound Host object.
    # @raise [KeyError] when the group or the member is not bound.
    def lookup(target)
      group_name, member_name = target.to_s.split("::", 2)
      group = @groups[group_name]
      raise KeyError, "no service group named #{group_name.inspect}" if group.nil?
      raise KeyError, "no member #{target.inspect} bound in registry" unless member_name

      group.fetch(member_name)
    end

    # @param target [String] two-level path with `::` separator.
    # @return [Boolean] whether +target+ resolves to a bound member.
    def bound?(target)
      group_name, member_name = target.to_s.split("::", 2)
      return false if member_name.nil?

      group = @groups[group_name]
      !group.nil? && !group[member_name].nil?
    end

    # @return [Array<Kobako::Registry::ServiceGroup>] all declared groups.
    def groups
      @groups.values
    end

    # @return [Integer] number of declared groups.
    def size
      @groups.size
    end

    # @return [Boolean] whether any groups have been declared.
    def empty?
      @groups.empty?
    end

    # Structured Frame 1 description (msgpack-encoded). Called by
    # `Sandbox#run` when assembling stdin Frame 1 (SPEC.md §Sandbox#run
    # 實作要點, step 1).
    #
    # @return [Array<Array(String, Array<String>)>] unencoded preamble array.
    def to_preamble
      @groups.values.map(&:to_preamble)
    end

    # Encode the preamble as msgpack bytes for stdin Frame 1 delivery.
    #
    # Uses plain MessagePack (no kobako ext types) because the preamble
    # contains only strings — no Handles or Exception envelopes. Structure:
    # `[["GroupName", ["MemberA", "MemberB"]], ...]` (SPEC.md §Sandbox#run
    # 實作要點, step 1).
    #
    # @return [String] binary msgpack bytes.
    def guest_preamble
      MessagePack.pack(to_preamble)
    end

    # Mark the Registry as sealed. Called by `Sandbox#run` on first run.
    # After sealing, #define raises ArgumentError. Idempotent.
    def seal!
      @sealed = true
      self
    end

    # @return [Boolean] whether {#seal!} has been called.
    def sealed?
      @sealed
    end

    # Reset the HandleTable for a new #run boundary. Called by Sandbox#run
    # before each invocation (SPEC.md §HandleTable 實作要點, #reset!).
    def reset_handles!
      @handle_table.reset!
    end

    # Dispatch a single RPC request and return the encoded response bytes.
    #
    # Called by the Rust ext from inside `__kobako_rpc_call`. Always returns
    # a binary String — never raises. Any failure during decode, lookup, or
    # method invocation is reified as a Response.err envelope so the guest
    # sees the failure as a normal RPC error rather than a wasm trap
    # (SPEC.md §Registry 實作要點 §dispatch 流程).
    #
    # @param request_bytes [String] msgpack-encoded Request envelope.
    # @return [String] msgpack-encoded Response envelope (binary).
    def dispatch(request_bytes)
      encode_ok(wrap_return(perform_dispatch(request_bytes)))
    rescue => e # rubocop:disable Style/RescueStandardError
      encode_dispatch_error(e)
    end

    def encode_dispatch_error(error)
      case error
      when Kobako::Wire::Error        then encode_err("runtime", "wire decode failed: #{error.message}")
      when DisconnectedTargetError    then encode_err("disconnected", error.message)
      when UndefinedTargetError       then encode_err("undefined", error.message)
      when ArgumentError              then encode_err("argument", error.message)
      else                                 encode_err("runtime", "#{error.class}: #{error.message}")
      end
    end

    # Expose the HandleTable for tests and wire-layer Handle wrapping.
    # @return [Kobako::Registry::HandleTable]
    attr_reader :handle_table

    private

    # Internal sentinel — raised when target resolution fails. Mapped to
    # Response.err with type="undefined". Not part of the public Kobako error
    # taxonomy because the failure is contained at the wire boundary.
    class UndefinedTargetError < StandardError; end

    # Internal sentinel — raised when a Handle target resolves to the
    # `:disconnected` sentinel in the HandleTable (ABA protection, SPEC.md
    # E-14). Mapped to Response.err with type="disconnected". Not part of
    # the public Kobako error taxonomy; failure is contained at the wire
    # boundary.
    class DisconnectedTargetError < StandardError; end

    def perform_dispatch(request_bytes)
      request = Kobako::Wire::Envelope.decode_request(request_bytes)
      target_object = resolve_target(request.target)
      args = request.args.map { |v| resolve_arg(v) }
      kwargs = request.kwargs.transform_values { |v| resolve_arg(v) }
      invoke(target_object, request.method_name, args, kwargs)
    end

    # SPEC.md B-16 — A Wire::Handle arriving as a positional or keyword
    # argument identifies a host-side object previously allocated by a prior
    # RPC's Handle wrap (B-14). Resolve it back to the Ruby object before
    # the dispatch reaches `public_send`. A Handle whose entry is the
    # `:disconnected` sentinel (E-14) raises DisconnectedTargetError so
    # the dispatcher emits a Response.err with type="disconnected".
    def resolve_arg(value)
      case value
      when Kobako::Wire::Handle
        fetch_live_object(value.id)
      else
        value
      end
    end

    # Resolve a Request target to the Ruby object the Registry (or
    # HandleTable) holds. String targets go through the Registry;
    # Handle targets (ext 0x01) go through the HandleTable.
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
      lookup(path)
    rescue KeyError => e
      raise UndefinedTargetError, e.message
    end

    def resolve_handle(handle)
      fetch_live_object(handle.id)
    end

    # Resolve +id+ through the HandleTable, distinguishing the
    # `:disconnected` sentinel (E-14) from an unknown id (E-13).
    def fetch_live_object(id)
      object = @handle_table.fetch(id)
      raise DisconnectedTargetError, "Handle id #{id} is disconnected" if object == :disconnected

      object
    rescue Kobako::HandleTableError => e
      raise UndefinedTargetError, e.message
    end

    # Invoke +method+ on +target+ with positional args and string-keyed
    # kwargs (symbolized at the boundary per SPEC.md B-12 Notes).
    #
    # Empty kwargs is wire-uniform (SPEC.md line 815: "empty kwargs is
    # encoded as empty map `0x80`, never absent"). Methods whose signature
    # accepts no keyword arguments must still dispatch when the wire carries
    # an empty kwargs map; the empty-kwargs branch omits the `**` splat so
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

    # SPEC.md B-14 — When a bound Service method returns a value that is not
    # wire-representable per B-13's type mapping, the wire layer routes it
    # through the HandleTable and the guest receives a Capability Handle in
    # place of the raw object.
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
  end
end
