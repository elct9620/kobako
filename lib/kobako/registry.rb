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
    # +name+ is a constant-form name as a +Symbol+ or +String+ (must satisfy
    # +NAME_PATTERN+). Returns the +Kobako::Registry::ServiceGroup+ for that
    # name, creating it if it does not exist. Raises +ArgumentError+ when
    # +name+ is malformed, or when called after the owning Sandbox has been
    # sealed by +#run+.
    def define(name)
      raise ArgumentError, "cannot define after Sandbox#run has been invoked" if @sealed

      name_str = name.to_s
      unless NAME_PATTERN.match?(name_str)
        raise ArgumentError,
              "GroupName must match #{NAME_PATTERN.inspect} (got #{name.inspect})"
      end

      @groups[name_str] ||= ServiceGroup.new(name_str)
    end

    # Resolve a +target+ path of the form +"GroupName::MemberName"+ to the
    # bound Host object. +target+ is a two-level path using the +::+
    # separator. Returns the bound Host object. Raises +KeyError+ when the
    # group or the member is not bound.
    def lookup(target)
      group_name, member_name = target.to_s.split("::", 2)
      group = @groups[group_name]
      raise KeyError, "no service group named #{group_name.inspect}" if group.nil?
      raise KeyError, "no member #{target.inspect} bound in registry" unless member_name

      group.fetch(member_name)
    end

    # Returns +true+ when +target+ (a +"GroupName::MemberName"+ path) resolves
    # to a bound member, +false+ otherwise.
    def bound?(target)
      group_name, member_name = target.to_s.split("::", 2)
      return false if member_name.nil?

      group = @groups[group_name]
      !group.nil? && !group[member_name].nil?
    end

    # Returns all declared +Kobako::Registry::ServiceGroup+ instances as an
    # +Array+.
    def groups
      @groups.values
    end

    # Returns the number of declared groups as an +Integer+.
    def size
      @groups.size
    end

    # Returns +true+ when no groups have been declared, +false+ otherwise.
    def empty?
      @groups.empty?
    end

    # Structured Frame 1 description. Called by +Sandbox#run+ when assembling
    # stdin Frame 1 ({SPEC.md §B-02}[link:../../SPEC.md]). Returns an
    # unencoded preamble array — an +Array+ of two-element +[name, members]+
    # arrays, one per declared group.
    def to_preamble
      @groups.values.map(&:to_preamble)
    end

    # Encode the preamble as msgpack bytes for stdin Frame 1 delivery
    # ({SPEC.md §B-02}[link:../../SPEC.md]). Uses plain MessagePack (no
    # kobako ext types) because the preamble contains only strings — no
    # Handles or Exception envelopes. Structure:
    # +[["GroupName", ["MemberA", "MemberB"]], ...]+. Returns a binary
    # +String+ of msgpack bytes.
    def guest_preamble
      MessagePack.pack(to_preamble)
    end

    # Mark the Registry as sealed. Called by `Sandbox#run` on first run.
    # After sealing, #define raises ArgumentError. Idempotent.
    def seal!
      @sealed = true
      self
    end

    # Returns +true+ when {#seal!} has been called, +false+ otherwise.
    def sealed?
      @sealed
    end

    # Reset the HandleTable for a new +#run+ boundary. Called by +Sandbox#run+
    # before each invocation ({SPEC.md §B-19}[link:../../SPEC.md]).
    def reset_handles!
      @handle_table.reset!
    end

    # Dispatch a single RPC request and return the encoded response bytes
    # ({SPEC.md §B-12}[link:../../SPEC.md]). +request_bytes+ is a
    # msgpack-encoded Request envelope. Called by the Rust ext from inside
    # +__kobako_rpc_call+. Always returns a binary +String+ — never raises.
    # Delegates to +Dispatcher.dispatch+ which reifies any failure as a
    # +Response.err+ envelope so the guest sees the failure as a normal RPC
    # error rather than a wasm trap.
    def dispatch(request_bytes)
      Dispatcher.dispatch(request_bytes, self)
    end

    # Expose the +Kobako::Registry::HandleTable+ for tests and wire-layer
    # Handle wrapping.
    attr_reader :handle_table
  end
end
