# frozen_string_literal: true

require_relative "handle"
require_relative "codec"

module Kobako
  # Host-side value object for a single +Sandbox#run+ invocation
  # ({docs/wire-codec.md Invocation channels}[link:../../docs/wire-codec.md];
  # {docs/behavior.md B-31}[link:../../docs/behavior.md]).
  #
  # An Invocation captures the host-layer concept of "a single +#run+
  # call": the entrypoint constant name plus its positional and keyword
  # arguments. Host pre-flight (E-24 / E-25 / E-29 / E-30) is enforced at
  # construction so the Value Object is the single source of truth —
  # anything that passes +Invocation.new+ is safe to encode and ship to
  # the guest.
  #
  # Invocation sits at top level, not under +Kobako::RPC+: RPC in SPEC is
  # the guest→host capability channel (Server / Client / Request /
  # Response / Handle); Invocation is the opposite direction (host→guest
  # entrypoint dispatch) and structurally rejects Handles (E-29), so it
  # has no relationship with the HandleTable. The +#encode+ output is the
  # "Invocation envelope" that ships through the +__kobako_run+ command
  # buffer.
  #
  # Built on the +class X < Data.define(...)+ subclass form (the
  # Steep-friendly shape — see +lib/kobako/outcome/panic.rb+).
  class Invocation < Data.define(:entrypoint, :args, :kwargs)
    # Ruby constant-name pattern enforced on the +entrypoint+ Symbol
    # ({docs/behavior.md E-25}[link:../../docs/behavior.md]). Parallel to
    # +Kobako::Snippet::Table::NAME_PATTERN+; the two constants name the
    # same regex but cover distinct surfaces (snippet identity vs.
    # entrypoint resolution) so a future divergence stays local.
    NAME_PATTERN = /\A[A-Z]\w*\z/

    # steep:ignore:start
    def initialize(entrypoint:, args: [], kwargs: {})
      super(
        entrypoint: normalize_entrypoint(entrypoint),
        args: validate_args!(args),
        kwargs: validate_kwargs!(kwargs)
      )
    end
    # steep:ignore:end

    # Encode this Invocation to the msgpack bytes the guest's
    # +__kobako_run+ entry point consumes as its command-buffer payload
    # ({docs/wire-codec.md Invocation channels}[link:../../docs/wire-codec.md]).
    # The Value Object's own invariants are the contract; this method
    # does not re-check the shape. Layout: msgpack map with string keys
    # +"entrypoint"+ (Symbol via ext 0x00), +"args"+ (Array), +"kwargs"+
    # (Map with Symbol keys).
    def encode
      Codec::Encoder.encode(
        "entrypoint" => entrypoint,
        "args" => args,
        "kwargs" => kwargs
      )
    end

    private

    # steep:ignore:start
    # E-24: target must be a Symbol or String (TypeError, not
    # ArgumentError — the wrong-type case is a Host App programming
    # error before the invocation reaches the guest). E-25: after
    # +.to_s+ the value must match NAME_PATTERN (ArgumentError),
    # rejecting +::+-segmented names and any non-constant form.
    def normalize_entrypoint(target)
      unless target.is_a?(Symbol) || target.is_a?(String)
        raise TypeError, "Invocation entrypoint must be a Symbol or String, got #{target.class}"
      end

      target_str = target.to_s
      unless NAME_PATTERN.match?(target_str)
        raise ArgumentError,
              "Invocation entrypoint must match #{NAME_PATTERN.inspect} (got #{target.inspect})"
      end

      target_str.to_sym
    end

    # E-29: +args+ must not contain a +Kobako::Handle+. Handles
    # are per-invocation and cannot enter the next invocation through
    # a control-plane channel; a guest that needs to call into a
    # stateful host object must obtain a fresh Handle through a
    # Service RPC inside the dispatched entrypoint.
    def validate_args!(args)
      raise ArgumentError, "Invocation args must be Array" unless args.is_a?(Array)
      raise ArgumentError, "Invocation args must not contain a Kobako::Handle" if args.any?(Kobako::Handle)

      args
    end

    # E-30: +kwargs+ keys must be Symbols, mirroring the wire codec's
    # Request kwargs rule. Validation lives here (not in the codec) so
    # the Host App sees the host-side error message before any encode
    # / decode boundary.
    def validate_kwargs!(kwargs)
      raise ArgumentError, "Invocation kwargs must be Hash" unless kwargs.is_a?(Hash)

      bad_keys = kwargs.each_key.grep_v(Symbol)
      unless bad_keys.empty?
        raise ArgumentError,
              "Invocation kwargs keys must be Symbols (got #{bad_keys.inspect})"
      end

      kwargs
    end
    # steep:ignore:end
  end
end
