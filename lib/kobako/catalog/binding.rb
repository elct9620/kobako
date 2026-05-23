# frozen_string_literal: true

require "msgpack"
require_relative "handler"
require_relative "../errors"
require_relative "../transport/request"
require_relative "binding/namespace"

module Kobako
  module Catalog
    # Kobako::Catalog::Binding — per-Sandbox host-side namespace registry.
    # Holds the Namespace / Member bindings and the preamble emitted on
    # Frame 1 ({docs/behavior.md B-07..B-11}[link:../../../docs/behavior.md]).
    #
    # Public API:
    #
    #   binding = Kobako::Catalog::Binding.new
    #   namespace = binding.define(:MyService)  # => Kobako::Catalog::Binding::Namespace
    #   namespace.bind(:KV, kv_object)          # => namespace (chainable)
    #   binding.to_preamble                     # => array for Frame 1
    #
    # Namespaces live at +Kobako::Catalog::Binding::Namespace+. Per-dispatch
    # routing is +Kobako::Transport::Dispatcher+'s responsibility — the
    # Dispatcher receives this Binding and the +Catalog::Handler+ as
    # arguments from the +Runtime#on_dispatch+ Proc that
    # +Kobako::Sandbox#initialize+ installs
    # ({docs/behavior.md B-12}[link:../../../docs/behavior.md]). The
    # Binding holds an injected +Catalog::Handler+ reference so dispatch
    # target resolution and host→guest auto-wrap share the same
    # Sandbox-owned allocator (docs/behavior.md B-19).
    class Binding
      # Build a fresh Binding. +handler+ is an internal seam that injects
      # a pre-configured +Catalog::Handler+; tests pass one whose +next_id+
      # is pinned near +MAX_ID+ to exercise the B-21 cap-exhaustion path
      # without 2³¹ allocations. Production callers leave it at the default.
      def initialize(handler: Catalog::Handler.new)
        @namespaces = {} # : Hash[String, Kobako::Catalog::Binding::Namespace]
        @handler = handler
        @sealed = false
      end

      # Declare or retrieve the Namespace named +name+ (idempotent — docs/behavior.md B-10).
      # +name+ is a constant-form name as a +Symbol+ or +String+ (must satisfy
      # +Namespace::NAME_PATTERN+). Returns the
      # +Kobako::Catalog::Binding::Namespace+ for that name, creating it if it
      # does not exist. Raises +ArgumentError+ when +name+ is malformed, or
      # when called after the owning Sandbox has been sealed by its first
      # invocation ({docs/behavior.md B-07}[link:../../../docs/behavior.md]).
      def define(name)
        raise ArgumentError, "cannot define after first Sandbox invocation" if @sealed

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
        namespace_name, member_name = target.to_s.split("::", 2)
        namespace = @namespaces[namespace_name]
        raise KeyError, "no namespace named #{namespace_name.inspect}" if namespace.nil?
        raise KeyError, "no member #{target.inspect} bound on binding" unless member_name

        namespace.fetch(member_name)
      end

      # Encode the preamble as msgpack bytes for stdin Frame 1 delivery
      # ({docs/behavior.md B-02}[link:../../../docs/behavior.md]). Uses plain MessagePack (no
      # kobako ext types) because the preamble contains only strings — no
      # Handles or Fault envelopes. Structure:
      # +[["Namespace", ["MemberA", "MemberB"]], ...]+. Returns a binary
      # +String+ of msgpack bytes.
      def encoded_preamble
        MessagePack.pack(@namespaces.values.map(&:to_preamble))
      end

      # Mark the Binding as sealed. Called by +Sandbox+ on the first
      # invocation. After sealing, #define raises ArgumentError. Idempotent.
      def seal!
        @sealed = true
        self
      end

      # Returns +true+ when {#seal!} has been called, +false+ otherwise.
      def sealed?
        @sealed
      end
    end
  end
end
