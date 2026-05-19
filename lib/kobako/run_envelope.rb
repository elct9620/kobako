# frozen_string_literal: true

require_relative "codec"
require_relative "rpc/handle"
require_relative "snippet_table"

module Kobako
  # Kobako::RunEnvelope — host-side validator + msgpack encoder for the
  # invocation envelope that +Sandbox#run+ delivers to the guest via the
  # +__kobako_run+ entrypoint
  # ({docs/wire-codec.md Invocation channels}[link:../../docs/wire-codec.md];
  # {docs/behavior.md B-31}[link:../../docs/behavior.md]).
  #
  # Owns the host pre-flight checks (E-24 / E-25 / E-29 / E-30) so the
  # malformed-input surface is enforced before the Sandbox seals or the
  # invocation reaches the guest. Anything that passes the constructor is
  # safe to encode and ship.
  class RunEnvelope
    attr_reader :target_sym, :args, :kwargs

    # Build a validated envelope. +target+ is a Symbol or String matching
    # the constant pattern (E-24 rejects other types as +TypeError+; E-25
    # rejects a bad name as +ArgumentError+). +args+ is the positional
    # argument array; a +Kobako::RPC::Handle+ inside it raises
    # +ArgumentError+ (E-29 — Handles are per-invocation and cannot enter
    # the next invocation through a control-plane channel). +kwargs+ is
    # the keyword argument Hash; a non-Symbol key raises +ArgumentError+
    # (E-30, mirroring the wire codec's kwargs key rule).
    def initialize(target, args, kwargs)
      @target_sym = normalize_target(target)
      check_args!(args)
      check_kwargs!(kwargs)
      @args = args
      @kwargs = kwargs
    end

    # Encode the validated envelope as msgpack bytes for delivery through
    # the guest's command buffer. Layout: msgpack map with string keys
    # +"entrypoint"+ (Symbol via ext 0x00), +"args"+ (Array), +"kwargs"+
    # (Map with Symbol keys).
    def encode
      Codec::Encoder.encode(
        "entrypoint" => @target_sym,
        "args" => @args,
        "kwargs" => @kwargs
      )
    end

    private

    def normalize_target(target)
      raise TypeError, "#run target must be a Symbol or String, got #{target.class}" \
        unless target.is_a?(Symbol) || target.is_a?(String)

      target_str = target.to_s
      unless SnippetTable::NAME_PATTERN.match?(target_str)
        raise ArgumentError,
              "#run target must match #{SnippetTable::NAME_PATTERN.inspect} (got #{target.inspect})"
      end

      target_str.to_sym
    end

    def check_args!(args)
      return unless args.any?(RPC::Handle)

      raise ArgumentError, "#run args must not contain a Kobako::RPC::Handle"
    end

    def check_kwargs!(kwargs)
      bad_keys = kwargs.each_key.grep_v(Symbol)
      return if bad_keys.empty?

      raise ArgumentError, "#run kwargs keys must be Symbols (got #{bad_keys.inspect})"
    end
  end
end
