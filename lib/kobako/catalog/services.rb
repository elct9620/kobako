# frozen_string_literal: true

require_relative "handles"
require_relative "../codec"
require_relative "../errors"

module Kobako
  module Catalog
    # Kobako::Catalog::Services — per-Sandbox registry of Service
    # bindings keyed by their constant-path name. Holds the flat
    # path→object table and the preamble emitted on Frame 1.
    #
    # Public API:
    #
    #   services = Kobako::Catalog::Services.new
    #   services.bind("MyService::KV", kv_object)  # => services (chainable)
    #   services.encode                            # => msgpack bytes for Frame 1
    #   services.lookup("MyService::KV")           # => kv_object
    #
    # Per-dispatch routing is +Kobako::Transport::Dispatcher+'s
    # responsibility — the Dispatcher receives this registry and the
    # +Catalog::Handles+ as arguments from the +Runtime#on_dispatch+ Proc
    # that +Kobako::Sandbox#initialize+ installs. The registry holds an
    # injected +Catalog::Handles+ reference so dispatch target resolution
    # and host→guest auto-wrap share the same Sandbox-owned allocator.
    class Services
      # Ruby constant-name pattern each +::+-separated bind-path segment
      # must match.
      NAME_PATTERN = /\A[A-Z]\w*\z/

      # Build a fresh registry. +handler+ is an internal seam that injects
      # a pre-configured +Catalog::Handles+; tests pass one whose +next_id+
      # is pinned near +MAX_ID+ to exercise the cap-exhaustion path
      # without 2³¹ allocations. Production callers leave it at the default.
      def initialize(handler: Catalog::Handles.new)
        @bindings = {} # : Hash[String, untyped]
        @handler = handler
        @sealed = false
        @encoded = nil # : String?
      end

      # Bind +object+ as the Service reachable at +path+ — a +Symbol+ or
      # +String+ of one or more +::+-separated constant-form segments
      # (+"MyService::KV"+ or a top-level +"File"+). Returns +self+ for
      # chaining. Raises +ArgumentError+ when a segment is malformed, when
      # +path+ collides with an existing binding (a name is a bound Service
      # or a grouping prefix, never both), or when the owning Sandbox has
      # been sealed by its first invocation.
      def bind(path, object)
        raise ArgumentError, "cannot bind after first Sandbox invocation" if @sealed

        path_str = validate_path!(path)
        raise ArgumentError, "Service path #{path_str} conflicts with an existing binding" if collision?(path_str)

        @bindings[path_str] = object
        self
      end

      # Resolve a +target+ constant path to the bound Service. Raises
      # +KeyError+ when no Service is bound at +target+.
      def lookup(target)
        target_str = target.to_s
        raise KeyError, "no service bound at #{target_str.inspect}" unless @bindings.key?(target_str)

        @bindings[target_str]
      end

      # Encode the preamble as msgpack bytes for stdin Frame 1 delivery —
      # a flat array of the bound constant paths, in bind order:
      # +["MyService::KV", "File"]+. Routes through Kobako::Codec::Encoder
      # like every other host-side wire encode; the preamble carries only
      # Strings, so none of the kobako ext types fire. Returns a binary
      # +String+ of msgpack bytes.
      #
      # Once sealed, the bytes are computed once and reused for every
      # subsequent invocation: sealing freezes Service registration at the
      # first invocation, so a bind reaching the registry after the seal
      # raises +ArgumentError+ and never alters Frame 1.
      def encode
        return @encoded if @encoded

        bytes = Codec::Encoder.encode(@bindings.keys).freeze
        @encoded = bytes if @sealed
        bytes
      end

      # Mark the registry as sealed. Called by +Sandbox+ on the first
      # invocation; afterwards #bind raises ArgumentError. Idempotent;
      # returns +self+.
      def seal!
        @sealed = true
        self
      end

      # Returns +true+ when #seal! has been called, +false+ otherwise.
      def sealed?
        @sealed
      end

      private

      def validate_path!(path)
        path_str = path.to_s
        segments = path_str.split("::", -1)
        return path_str if !segments.empty? && segments.all? { |seg| NAME_PATTERN.match?(seg) }

        raise ArgumentError,
              "bind path must be constant-form segments joined by '::' (got #{path.inspect})"
      end

      # A path collides when it equals, is a prefix of, or extends an
      # existing binding on the +::+ segment boundary — the guardrail that
      # keeps a name from being both a bound Service and a grouping prefix.
      def collision?(path)
        @bindings.each_key.any? do |existing|
          existing == path ||
            existing.start_with?("#{path}::") ||
            path.start_with?("#{existing}::")
        end
      end
    end
  end
end
