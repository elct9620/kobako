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
require_relative "registry/dispatcher"

module Kobako
  # Kobako::Registry — per-Sandbox container of Service Groups and Handle
  # state. Manages capability injection and guest-initiated RPC dispatch
  # ({SPEC.md §B-07..B-21}[link:../../SPEC.md]).
  #
  # Public API:
  #
  #   registry = Kobako::Registry.new
  #   group = registry.define(:MyService)    # => ServiceGroup
  #   group.bind(:KV, kv_object)             # => group (chainable)
  #   registry.to_preamble                   # => array for Frame 1
  #   registry.dispatch(request_bytes)       # => msgpack bytes (delegated to Dispatcher)
  #
  # Service Groups are defined in +Kobako::Registry::ServiceGroup+
  # (lib/kobako/registry/service_group.rb). The opaque Handle allocator lives
  # in +Kobako::Registry::HandleTable+ (lib/kobako/registry/handle_table.rb).
  # Dispatch helpers live in +Kobako::Registry::Dispatcher+
  # (lib/kobako/registry/dispatcher.rb).
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
    # Called by the Rust ext from inside +__kobako_rpc_call+. Always returns
    # a binary String — never raises. Delegates to +Dispatcher.dispatch+ which
    # reifies any failure as a Response.err envelope so the guest sees the
    # failure as a normal RPC error rather than a wasm trap
    # ({SPEC.md §Registry 實作要點 §dispatch 流程}[link:../../SPEC.md]).
    #
    # @param request_bytes [String] msgpack-encoded Request envelope.
    # @return [String] msgpack-encoded Response envelope (binary).
    def dispatch(request_bytes)
      Dispatcher.dispatch(request_bytes, self)
    end

    # Expose the HandleTable for tests and wire-layer Handle wrapping.
    # @return [Kobako::Registry::HandleTable]
    attr_reader :handle_table
  end
end
