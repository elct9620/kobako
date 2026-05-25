# frozen_string_literal: true

require_relative "../handle"
require_relative "../codec"
require_relative "../transport"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns the +Run+ envelope value object — the host→guest request shape
  # consumed by +__kobako_run+.
  module Transport
    # Host-side value object for a single +Sandbox#run+ invocation
    # ({docs/wire-codec.md Invocation channels}[link:../../../docs/wire-codec.md];
    # {docs/behavior.md B-31}[link:../../../docs/behavior.md]).
    #
    # A Run captures the host-layer concept of "a single +#run+
    # call": the entrypoint constant name plus its positional and keyword
    # arguments. Host pre-flight (E-24 / E-25 / E-29 / E-30) is enforced at
    # construction so the Value Object is the single source of truth —
    # anything that passes +Run.new+ is safe to encode and ship to
    # the guest.
    #
    # Run is the host→guest entrypoint dispatch envelope (the +#run+
    # request shape), the symmetric counterpart to the guest→host
    # +Request+ envelope. +#encode+ takes the Sandbox's
    # +Catalog::Handler+ and routes any non-wire-representable +args+ /
    # +kwargs+ leaf through it as a +Kobako::Handle+
    # ({docs/behavior.md B-34}[link:../../../docs/behavior.md]) — the
    # symmetric counterpart of the guest→host wrap path in the
    # dispatcher (B-14). A +Kobako::Handle+ that arrives **already
    # constructed** in the caller's +args+ / +kwargs+ is rejected at
    # construction (E-29): legitimate Handles only enter Host App code
    # through error fields, so a Handle reaching the call site is by
    # definition smuggled in. The +#encode+ output is the "Run envelope"
    # that ships through the +__kobako_run+ command buffer.
    #
    # Built on the +class X < Data.define(...)+ subclass form (the
    # Steep-friendly shape — see +lib/kobako/outcome/panic.rb+).
    class Run < Data.define(:entrypoint, :args, :kwargs)
      # Ruby constant-name pattern enforced on the +entrypoint+ Symbol
      # ({docs/behavior.md E-25}[link:../../../docs/behavior.md]). Parallel to
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
      # Walks +args+ / +kwargs+ through {Codec::Utils.deep_wrap} so any
      # non-wire-representable leaf is allocated into +handler+ and
      # replaced with a +Kobako::Handle+
      # ({docs/behavior.md B-34}[link:../../../docs/behavior.md]); the
      # +handler+ argument is the Sandbox's table, sharing the same
      # allocator the guest→host return path (B-14) uses.
      #
      # Layout: msgpack map with string keys +"entrypoint"+ (Symbol via
      # ext 0x00), +"args"+ (Array), +"kwargs"+ (Map with Symbol keys);
      # any wrapped leaf rides as ext 0x01 in its original position
      # (docs/wire-codec.md § ext 0x01 position rules).
      def encode(handler)
        Codec::Encoder.encode(
          "entrypoint" => entrypoint,
          "args" => Codec::Utils.deep_wrap(args, handler),
          "kwargs" => Codec::Utils.deep_wrap(kwargs, handler)
        )
      end

      private

      # E-24: target must be a Symbol or String (TypeError, not
      # ArgumentError — the wrong-type case is a Host App programming
      # error before the run reaches the guest). E-25: after +.to_s+
      # the value must match NAME_PATTERN (ArgumentError), rejecting
      # +::+-segmented names and any non-constant form.
      def normalize_entrypoint(target)
        unless target.is_a?(Symbol) || target.is_a?(String)
          raise TypeError, "Run entrypoint must be a Symbol or String, got #{target.class}"
        end

        target_str = target.to_s
        unless NAME_PATTERN.match?(target_str)
          raise ArgumentError,
                "Run entrypoint must match #{NAME_PATTERN.inspect} (got #{target.inspect})"
        end

        target_str.to_sym
      end

      # E-29: +args+ must not contain a +Kobako::Handle+. The Handle
      # allocator lives inside the Host Gem; legitimate paths surface
      # Handle objects only through raised error fields, so a Handle
      # reaching +args+ is a forged or smuggled token. Non-wire-
      # representable arguments that are not Handles are handled by
      # auto-wrap inside +#encode+ (B-34) — the reject path is reserved
      # for Handle objects specifically.
      def validate_args!(args)
        raise ArgumentError, "Run args must be Array" unless args.is_a?(Array)
        raise ArgumentError, forged_handle_message("args") if args.any?(Kobako::Handle)

        args
      end

      # E-30 covers the non-Symbol kwargs-key case; E-29 also rejects a
      # +Kobako::Handle+ arriving as a kwargs value (same forged-token
      # principle as the +args+ branch). Both checks live here so the
      # Host App sees the host-side error message before any encode /
      # decode boundary.
      def validate_kwargs!(kwargs)
        raise ArgumentError, "Run kwargs must be Hash" unless kwargs.is_a?(Hash)

        bad_keys = kwargs.each_key.grep_v(Symbol)
        unless bad_keys.empty?
          raise ArgumentError,
                "Run kwargs keys must be Symbols (got #{bad_keys.inspect})"
        end
        raise ArgumentError, forged_handle_message("kwargs values") if kwargs.each_value.any?(Kobako::Handle)

        kwargs
      end

      # Single source of truth for the E-29 reject message so the args
      # and kwargs branches stay phrased identically. Message stays in
      # caller vocabulary: it names the affected slot and the reason
      # without leaking SPEC anchor identifiers (B-xx / E-xx live in
      # source comments, not user-visible errors) or self-referential
      # architecture terms — the error is raised BY kobako, so saying
      # "allocated by the Host Gem" reads as third-person about self.
      def forged_handle_message(slot)
        "Run #{slot} must not contain a Kobako::Handle — " \
          "Kobako::Handle instances are internal wire tokens, not caller-constructible"
      end
    end
  end
end
