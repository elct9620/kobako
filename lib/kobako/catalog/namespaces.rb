# frozen_string_literal: true

require_relative "handles"
require_relative "../codec"
require_relative "../errors"
require_relative "../transport/request"
require_relative "../namespace"

module Kobako
  module Catalog
    # Kobako::Catalog::Namespaces — per-Sandbox registry of
    # +Kobako::Namespace+ entities. Holds the Namespace / Member bindings
    # and the preamble emitted on Frame 1
    # ({docs/behavior.md B-07..B-11}[link:../../../docs/behavior.md]).
    #
    # Public API:
    #
    #   namespaces = Kobako::Catalog::Namespaces.new
    #   namespace = namespaces.define(:MyService)  # => Kobako::Namespace
    #   namespace.bind(:KV, kv_object)             # => namespace (chainable)
    #   namespaces.encode                          # => msgpack bytes for Frame 1
    #   namespaces.lookup("MyService::KV")         # => kv_object
    #
    # Namespaces live at +Kobako::Namespace+. Per-dispatch routing is
    # +Kobako::Transport::Dispatcher+'s responsibility — the Dispatcher
    # receives this registry and the +Catalog::Handles+ as arguments from
    # the +Runtime#on_dispatch+ Proc that +Kobako::Sandbox#initialize+
    # installs ({docs/behavior.md B-12}[link:../../../docs/behavior.md]).
    # The registry holds an injected +Catalog::Handles+ reference so
    # dispatch target resolution and host→guest auto-wrap share the same
    # Sandbox-owned allocator (docs/behavior.md B-19).
    class Namespaces
      # Build a fresh registry. +handler+ is an internal seam that injects
      # a pre-configured +Catalog::Handles+; tests pass one whose +next_id+
      # is pinned near +MAX_ID+ to exercise the B-21 cap-exhaustion path
      # without 2³¹ allocations. Production callers leave it at the default.
      def initialize(handler: Catalog::Handles.new)
        @namespaces = {} # : Hash[String, Kobako::Namespace]
        @handler = handler
        @sealed = false
      end

      # Declare or retrieve the Namespace named +name+ (idempotent — docs/behavior.md B-10).
      # +name+ is a constant-form name as a +Symbol+ or +String+ (must satisfy
      # +Namespace::NAME_PATTERN+). Returns the +Kobako::Namespace+ for that
      # name, creating it if it does not exist. Raises +ArgumentError+ when
      # +name+ is malformed, or when called after the owning Sandbox has been
      # sealed by its first invocation
      # ({docs/behavior.md B-07}[link:../../../docs/behavior.md]).
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
        raise KeyError, "no member in target #{target.inspect}" unless member_name

        namespace.fetch(member_name)
      end

      # Encode the preamble as msgpack bytes for stdin Frame 1 delivery
      # ({docs/behavior.md B-02}[link:../../../docs/behavior.md]). Routes through
      # {Kobako::Codec::Encoder} like every other host-side wire encode so
      # there is a single codec path; the preamble carries only Strings and
      # Arrays, so none of the kobako ext types actually fire. Structure:
      # +[["Namespace", ["MemberA", "MemberB"]], ...]+. Returns a binary
      # +String+ of msgpack bytes.
      def encode
        Codec::Encoder.encode(@namespaces.values.map(&:to_preamble))
      end

      # Mark the registry as sealed. Called by +Sandbox+ on the first
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
