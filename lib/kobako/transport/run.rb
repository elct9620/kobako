# frozen_string_literal: true

require_relative "../handle"
require_relative "../codec"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns the +Run+ envelope value object — the host→guest request shape
  # consumed by +__kobako_run+.
  module Transport
    # Host-side value object for a single +Sandbox#run+ invocation
    # ({docs/wire-codec.md Invocation channels}[link:../../../docs/wire-codec.md]).
    #
    # A Run captures the host-layer concept of "a single +#run+
    # call": the entrypoint constant name plus its positional and keyword
    # arguments. Host pre-flight (entrypoint type / name pattern, forged
    # Handle, kwargs-key type) is enforced at construction so the Value
    # Object is the single source of truth — anything that passes
    # +Run.new+ is safe to encode and ship to the guest.
    #
    # Run is the host→guest entrypoint dispatch envelope (the +#run+
    # request shape), the symmetric counterpart to the guest→host
    # +Request+ envelope. +#encode+ takes the Sandbox's
    # +Catalog::Handles+ and routes any non-wire-representable +args+ /
    # +kwargs+ leaf through it as a +Kobako::Handle+ — the
    # symmetric counterpart of the guest→host wrap path in the
    # dispatcher. A +Kobako::Handle+ that arrives **already
    # constructed** in the caller's +args+ / +kwargs+ is rejected at
    # construction: legitimate Handles only enter Host App code
    # through error fields, so a Handle reaching the call site is by
    # definition smuggled in. The +#encode+ output is the "Run envelope"
    # that ships through the +__kobako_run+ command buffer.
    #
    # Built on the +class X < Data.define(...)+ subclass form (the
    # Steep-friendly shape — see +lib/kobako/outcome/panic.rb+).
    class Run < Data.define(:entrypoint, :args, :kwargs)
      # Ruby constant-name pattern enforced on the +entrypoint+ Symbol.
      # Parallel to
      # +Kobako::Catalog::Snippets::NAME_PATTERN+; the two constants name the
      # same regex but cover distinct surfaces (snippet identity vs.
      # entrypoint resolution) so a future divergence stays local.
      NAME_PATTERN = /\A[A-Z]\w*\z/

      def initialize(entrypoint:, args: [], kwargs: {})
        entrypoint = normalize_entrypoint(entrypoint)
        args = validate_args!(args)
        kwargs = validate_kwargs!(kwargs)
        super
      end

      # Encode this Run to the msgpack bytes the guest's +__kobako_run+
      # entry point consumes as its command-buffer payload
      # ({docs/wire-codec.md Invocation channels}[link:../../../docs/wire-codec.md]).
      # Walks +args+ / +kwargs+ through {Codec::HandleWalk.deep_wrap} so
      # any non-wire-representable leaf is allocated into +handler+ and
      # replaced with a +Kobako::Handle+; the
      # +handler+ argument is the Sandbox's table, sharing the same
      # allocator the guest→host return path uses.
      #
      # Layout: msgpack map with string keys +"entrypoint"+ (Symbol via
      # ext 0x00), +"args"+ (Array), +"kwargs"+ (Map with Symbol keys);
      # any wrapped leaf rides as ext 0x01 in its original position
      # (docs/wire-codec.md § ext 0x01 position rules).
      def encode(handler)
        Codec::Encoder.encode(
          "entrypoint" => entrypoint,
          "args" => Codec::HandleWalk.deep_wrap(args, handler),
          "kwargs" => Codec::HandleWalk.deep_wrap(kwargs, handler)
        )
      end

      private

      # The target must be a Symbol or String (TypeError, not
      # ArgumentError — the wrong-type case is a Host App programming
      # error before the run reaches the guest). After +.to_s+
      # the value must match NAME_PATTERN (ArgumentError), rejecting
      # +::+-segmented names and any non-constant form.
      def normalize_entrypoint(target)
        unless target.is_a?(Symbol) || target.is_a?(String)
          raise TypeError, "entrypoint must be a Symbol or String, got #{target.class}"
        end

        target_str = target.to_s
        unless NAME_PATTERN.match?(target_str)
          raise ArgumentError,
                "entrypoint must match #{NAME_PATTERN.inspect} (got #{target.inspect})"
        end

        target_str.to_sym
      end

      # +args+ must not contain a +Kobako::Handle+. The Handle
      # allocator lives inside the Host Gem; legitimate paths surface
      # Handle objects only through raised error fields, so a Handle
      # reaching +args+ is a forged or smuggled token. Non-wire-
      # representable arguments that are not Handles are handled by
      # auto-wrap inside +#encode+ — the reject path is reserved
      # for Handle objects specifically.
      def validate_args!(args)
        raise ArgumentError, "arguments must be an Array" unless args.is_a?(Array)
        raise ArgumentError, forged_handle_message("arguments") if args.any?(Kobako::Handle)

        args
      end

      # Reject a non-Symbol kwargs key, and a +Kobako::Handle+ arriving
      # as a kwargs value (same forged-token principle as the +args+
      # branch). Both checks live here so the Host App sees the
      # host-side error message before any encode / decode boundary.
      def validate_kwargs!(kwargs)
        raise ArgumentError, "keyword arguments must be a Hash" unless kwargs.is_a?(Hash)

        bad_keys = kwargs.each_key.grep_v(Symbol)
        unless bad_keys.empty?
          raise ArgumentError,
                "keyword argument keys must be Symbols (got #{bad_keys.inspect})"
        end
        raise ArgumentError, forged_handle_message("keyword argument values") if kwargs.each_value.any?(Kobako::Handle)

        kwargs
      end

      # Single source of truth for the forged-Handle reject message so the
      # args and kwargs branches stay phrased identically. Message stays in
      # caller vocabulary: it names the affected slot and the reason
      # without leaking internal SPEC identifiers or self-referential
      # architecture terms — the error is raised BY kobako, so saying
      # "allocated by the Host Gem" reads as third-person about self.
      def forged_handle_message(slot)
        "#{slot} must not contain a Kobako::Handle — " \
          "Handles are created internally by the Sandbox and cannot be passed in"
      end
    end
  end
end
