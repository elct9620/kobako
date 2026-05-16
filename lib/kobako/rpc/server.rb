# frozen_string_literal: true

require "msgpack"
require_relative "../errors"
require_relative "envelope"
require_relative "namespace"
require_relative "handle_table"
require_relative "dispatcher"

module Kobako
  module RPC
    # Kobako::RPC::Server — per-Sandbox host-side RPC coordinator. Maintains
    # the Namespace / Member registry, owns the HandleTable, and routes
    # incoming Requests to the resolved Service object
    # ({SPEC.md B-07..B-21}[link:../../../SPEC.md]).
    #
    # Public API:
    #
    #   server = Kobako::RPC::Server.new
    #   namespace = server.define(:MyService)    # => Kobako::RPC::Namespace
    #   namespace.bind(:KV, kv_object)           # => namespace (chainable)
    #   server.to_preamble                       # => array for Frame 1
    #   server.dispatch(request_bytes)           # => msgpack bytes (delegated to Dispatcher)
    #
    # Namespaces live at +Kobako::RPC::Namespace+
    # (lib/kobako/rpc/namespace.rb). The opaque Handle allocator lives at
    # +Kobako::RPC::HandleTable+
    # (lib/kobako/rpc/handle_table.rb). Dispatch helpers live at
    # +Kobako::RPC::Dispatcher+
    # (lib/kobako/rpc/dispatcher.rb).
    class Server
      # Build a fresh Server. +handle_table+ is an internal seam that
      # injects a pre-configured +HandleTable+; tests pass one whose +next_id+
      # is pinned near +MAX_ID+ to exercise the B-21 cap-exhaustion path
      # without 2³¹ allocations. Production callers leave it at the default.
      def initialize(handle_table: HandleTable.new)
        @namespaces = {} # : Hash[String, Kobako::RPC::Namespace]
        @handle_table = handle_table
        @sealed = false
      end

      # Declare or retrieve the Namespace named +name+ (idempotent — SPEC.md B-10).
      # +name+ is a constant-form name as a +Symbol+ or +String+ (must satisfy
      # +Namespace::NAME_PATTERN+). Returns the +Kobako::RPC::Namespace+ for
      # that name, creating it if it does not exist. Raises +ArgumentError+
      # when +name+ is malformed, or when called after the owning Sandbox has
      # been sealed by +#run+.
      def define(name)
        raise ArgumentError, "cannot define after Sandbox#run has been invoked" if @sealed

        name_str = name.to_s
        unless Namespace::NAME_PATTERN.match?(name_str)
          raise ArgumentError,
                "Namespace name must match #{Namespace::NAME_PATTERN.inspect} (got #{name.inspect})"
        end

        @namespaces[name_str] ||= Namespace.new(name_str)
      end

      # Resolve a +target+ path of the form +"Namespace::Member"+ to the
      # bound Host object. +target+ is a two-level path using the +::+
      # separator. Returns the bound Host object. Raises +KeyError+ when the
      # namespace or the member is not bound.
      def lookup(target)
        namespace, member_name, namespace_name = parse_target(target)
        raise KeyError, "no namespace named #{namespace_name.inspect}" if namespace.nil?
        raise KeyError, "no member #{target.inspect} bound on server" unless member_name

        namespace.fetch(member_name)
      end

      # Returns +true+ when +target+ (a +"Namespace::Member"+ path) resolves
      # to a bound member, +false+ otherwise.
      def bound?(target)
        namespace, member_name, = parse_target(target)
        !namespace.nil? && !member_name.nil? && !namespace[member_name].nil?
      end

      # Returns all declared +Kobako::RPC::Namespace+ instances as an +Array+.
      def namespaces
        @namespaces.values
      end

      # Returns the number of declared namespaces as an +Integer+.
      def size
        @namespaces.size
      end

      # Returns +true+ when no namespaces have been declared, +false+ otherwise.
      def empty?
        @namespaces.empty?
      end

      # Structured Frame 1 description. Called by +Sandbox#run+ when assembling
      # stdin Frame 1 ({SPEC.md B-02}[link:../../../SPEC.md]). Returns an
      # unencoded preamble array — an +Array+ of two-element +[name, members]+
      # arrays, one per declared namespace.
      def to_preamble
        @namespaces.values.map(&:to_preamble)
      end

      # Encode the preamble as msgpack bytes for stdin Frame 1 delivery
      # ({SPEC.md B-02}[link:../../../SPEC.md]). Uses plain MessagePack (no
      # kobako ext types) because the preamble contains only strings — no
      # Handles or Fault envelopes. Structure:
      # +[["Namespace", ["MemberA", "MemberB"]], ...]+. Returns a binary
      # +String+ of msgpack bytes.
      def encoded_preamble
        MessagePack.pack(to_preamble)
      end

      # Mark the Server as sealed. Called by +Sandbox#run+ on first run.
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
      # before each invocation ({SPEC.md B-19}[link:../../../SPEC.md]).
      def reset_handles!
        @handle_table.reset!
      end

      # Dispatch a single RPC request and return the encoded response bytes
      # ({SPEC.md B-12}[link:../../../SPEC.md]). +request_bytes+ is a
      # msgpack-encoded Request envelope. Called by the Rust ext from inside
      # +__kobako_dispatch+. Always returns a binary +String+ — never raises.
      # Delegates to +Dispatcher.dispatch+ which reifies any failure as a
      # +Response.error+ envelope so the guest sees the failure as a normal RPC
      # error rather than a wasm trap.
      def dispatch(request_bytes)
        Dispatcher.dispatch(request_bytes, self)
      end

      # Expose the +HandleTable+ for tests and wire-layer Handle wrapping.
      attr_reader :handle_table

      private

      # Split +target+ on the +::+ separator and resolve the namespace half.
      # Returns +[namespace_or_nil, member_str_or_nil, namespace_name_str]+ so
      # each public method ({#lookup} / {#bound?}) only owns its boundary
      # semantics (raise vs predicate).
      def parse_target(target)
        namespace_name, member_name = target.to_s.split("::", 2)
        [@namespaces[namespace_name], member_name, namespace_name]
      end
    end
  end
end
