# frozen_string_literal: true

require_relative "outcome/panic"

module Kobako
  # Host-facing boundary for the OUTCOME_BUFFER produced by
  # +__kobako_run+. Takes raw outcome bytes — a one-byte tag followed by
  # the msgpack-encoded body — and maps them to either the unwrapped
  # mruby return value or a raised three-layer
  # ({SPEC.md "Error Scenarios"}[link:../../SPEC.md]) exception.
  #
  # Self-contained: this module owns the wire framing (tag bytes,
  # body decoding), and the +Panic+ wire record lives at
  # +Kobako::Outcome::Panic+. The byte-level msgpack codec at
  # +Kobako::Codec+ is invoked for the body itself; otherwise
  # nothing in +RPC+ participates.
  #
  #   * tag 0x01, decode OK                 → return decoded value
  #   * tag 0x01, decode fails              → SandboxError (E-09)
  #   * tag 0x02, origin="service"          → ServiceError (E-13)
  #   * tag 0x02, origin="sandbox"/missing  → SandboxError (E-04..E-07)
  #   * tag 0x02, decode fails              → SandboxError (E-08)
  #   * unknown tag                         → TrapError    (E-03)
  module Outcome
    # First byte of the OUTCOME_BUFFER for the success branch — body is
    # the bare msgpack encoding of the returned value
    # ({SPEC.md Outcome Envelope}[link:../../SPEC.md]).
    TYPE_VALUE = 0x01
    # First byte of the OUTCOME_BUFFER for the failure branch — body is
    # the msgpack Panic map.
    TYPE_PANIC = 0x02

    module_function

    def decode(bytes)
      tag, body = split_outcome_tag(bytes)
      case tag
      when TYPE_VALUE
        decode_value(body)
      when TYPE_PANIC
        decode_panic(body)
      else
        raise trap_for_tag(tag)
      end
    end

    # TrapError for unknown or absent tag
    # ({SPEC.md ABI Signatures}[link:../../SPEC.md]: len=0 and unknown-tag
    # both walk the trap path).
    def trap_for_tag(tag)
      return TrapError.new("guest exited without writing an outcome (len=0)") if tag.nil?

      TrapError.new(format("unknown outcome tag 0x%<tag>02x", tag: tag))
    end

    def split_outcome_tag(bytes)
      bytes = bytes.b
      return [nil, "".b] if bytes.empty?

      tag = bytes.getbyte(0) # : Integer
      body = bytes.byteslice(1, bytes.bytesize - 1) # : String
      [tag, body]
    end

    # Decode failure on the success tag is a SandboxError (E-09): the
    # framing was fine, but the carried value is unrepresentable.
    def decode_value(body)
      Kobako::Codec::Decoder.decode(body)
    rescue Kobako::Codec::Error => e
      raise wire_violation_error("result envelope decode failed: #{e.message}")
    end

    # Decode failure on the panic tag is a SandboxError (E-08). Either
    # path raises — on success the decoded Panic is mapped to its three-
    # layer exception via +build_panic_error+ and raised; on wire-decode
    # failure the rescue path raises the wire-violation +SandboxError+.
    def decode_panic(body)
      raise build_panic_error(decode_panic_map(body))
    rescue Kobako::Codec::Error => e
      raise wire_violation_error("panic envelope decode failed: #{e.message}")
    end

    # Build a +Panic+ value object from the msgpack-decoded body. Raises
    # +Kobako::Codec::InvalidType+ when the body is not a map or
    # when required keys are missing — both routed by +decode_panic+ to
    # +wire_violation_error+.
    def decode_panic_map(body)
      map = Kobako::Codec::Decoder.decode(body)
      raise Kobako::Codec::InvalidType, "Panic envelope must be a map, got #{map.class}" unless map.is_a?(Hash)

      Kobako::Codec::Utils.translate_value_object_error do
        Panic.new(
          origin: map["origin"], klass: map["class"], message: map["message"],
          backtrace: map["backtrace"] || [], details: map["details"]
        )
      end
    end

    # Map a decoded Panic record into the corresponding three-layer
    # Ruby exception. +origin == "service"+ → ServiceError (with the
    # +::Disconnected+ subclass selected when the panic carries the
    # disconnected sentinel —
    # {SPEC "Error Classes"}[link:../../SPEC.md]); everything else
    # → SandboxError.
    def build_panic_error(panic)
      panic_target_class(panic).new(
        panic.message,
        origin: panic.origin,
        klass: panic.klass,
        backtrace_lines: panic.backtrace,
        details: panic.details
      )
    end

    # {SPEC "Error Classes"}[link:../../SPEC.md]: when
    # +origin="service"+ and the panic +class+ field names
    # +ServiceError::Disconnected+, surface that subclass so callers can
    # rescue the disconnected path specifically (E-14).
    def panic_target_class(panic)
      return SandboxError unless panic.origin == Panic::ORIGIN_SERVICE

      panic.klass == "Kobako::ServiceError::Disconnected" ? ServiceError::Disconnected : ServiceError
    end

    def wire_violation_error(message)
      SandboxError.new(
        message,
        origin: Panic::ORIGIN_SANDBOX,
        klass: "Kobako::RPC::WireError"
      )
    end
  end
end
