# frozen_string_literal: true

require_relative "outcome/panic"
require_relative "transport/wire_error"

module Kobako
  # Host-facing boundary for the OUTCOME_BUFFER produced by
  # +__kobako_eval+. Takes raw outcome bytes — a one-byte tag followed by
  # the msgpack-encoded body — and maps them to either the unwrapped
  # mruby return value or a raised three-layer
  # ({docs/behavior.md Error Scenarios}[link:../../docs/behavior.md]) exception.
  #
  # Self-contained: this module owns the wire framing (tag bytes,
  # body decoding), and the +Panic+ wire record lives at
  # +Kobako::Outcome::Panic+. The byte-level msgpack codec at
  # +Kobako::Codec+ is invoked for the body itself; otherwise
  # nothing in +Transport+ participates.
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
    # ({docs/wire-contract.md Outcome Envelope}[link:../../docs/wire-contract.md]).
    TYPE_VALUE = 0x01
    # First byte of the OUTCOME_BUFFER for the failure branch — body is
    # the msgpack Panic map.
    TYPE_PANIC = 0x02

    module_function

    def decode(bytes)
      tag, body = split_tag(bytes)
      case tag
      when TYPE_VALUE
        decode_value(body)
      when TYPE_PANIC
        decode_panic(body)
      else
        raise build_trap_error(tag)
      end
    end

    # TrapError for unknown or absent tag
    # ({docs/wire-codec.md ABI Signatures}[link:../../docs/wire-codec.md]:
    # zero-length output and unrecognised first byte both walk the trap
    # path). The user-facing message stays in caller vocabulary — the
    # raw tag byte (or absence) belongs in +details+ for operators, not
    # in the message a caller sees.
    def build_trap_error(tag)
      if tag.nil?
        TrapError.new("Sandbox exited without producing a result")
      else
        TrapError.new(
          "Sandbox produced an unrecognised result; the runtime is corrupted, " \
          "discard this Sandbox before another invocation"
        )
      end
    end

    def split_tag(bytes)
      bytes = bytes.b
      return [nil, "".b] if bytes.empty?

      tag = bytes.getbyte(0) # : Integer
      body = bytes.byteslice(1, bytes.bytesize - 1) # : String
      [tag, body]
    end

    # Decode failure on the success tag is a SandboxError (E-09): the
    # framing was fine, but the carried value is unrepresentable. The
    # specific codec fault is stashed in +details[:wire_error]+ rather
    # than spliced into the message — callers cannot act on the inner
    # "Symbol payload must be …" wording, but operators triaging a
    # corrupted Sandbox runtime still need it.
    def decode_value(body)
      Kobako::Codec::Decoder.decode(body)
    rescue Kobako::Codec::Error => e
      raise build_wire_violation_error(
        "Sandbox produced an invalid result value",
        wire_error: e.message
      )
    end

    # Decode failure on the panic tag is a SandboxError (E-08). Either
    # path raises — on success the decoded Panic is mapped to its three-
    # layer exception via +build_panic_error+ and raised; on wire-decode
    # failure the rescue path raises the wire-violation +SandboxError+.
    def decode_panic(body)
      raise build_panic_error(parse_panic(body))
    rescue Kobako::Codec::Error => e
      raise build_wire_violation_error(
        "Sandbox produced an invalid panic record",
        wire_error: e.message
      )
    end

    # Build a +Panic+ value object from the msgpack-decoded body. Raises
    # +Kobako::Codec::InvalidType+ when the body is not a map or
    # when required keys are missing — both routed by +decode_panic+ to
    # +build_wire_violation_error+. The +InvalidType+ message itself is
    # never user-facing; it lands in +details[:wire_error]+ via the
    # rescue chain above.
    def parse_panic(body)
      map = Kobako::Codec::Decoder.decode(body)
      raise Kobako::Codec::InvalidType, "panic body must be a map, got #{map.class}" unless map.is_a?(Hash)

      Kobako::Codec::Utils.wire_boundary do
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
    # {docs/behavior.md Error Classes}[link:../../docs/behavior.md]); everything else
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

    # {docs/behavior.md Error Classes}[link:../../docs/behavior.md]: map
    # the panic +class+ field to the matching Ruby exception subclass so
    # callers can rescue specific failure paths. +origin="service"+ plus
    # +class="Kobako::ServiceError::Disconnected"+ selects the
    # +Disconnected+ subclass (E-14); +origin="sandbox"+ plus
    # +class="Kobako::BytecodeError"+ selects the +BytecodeError+
    # subclass (E-37 / E-38). Everything else falls back to the base
    # class for the origin.
    def panic_target_class(panic)
      case panic.origin
      when Panic::ORIGIN_SERVICE
        panic.klass == "Kobako::ServiceError::Disconnected" ? ServiceError::Disconnected : ServiceError
      else
        panic.klass == "Kobako::BytecodeError" ? BytecodeError : SandboxError
      end
    end

    # Lift the wire-violation fallback to the real
    # +Kobako::Transport::WireError+ class so callers can +rescue+ it
    # specifically instead of pattern-matching on +error.klass+. The
    # +klass+ field is still populated so existing operator-side
    # tooling that greps on the string continues to work.
    # +wire_error+ carries the inner codec / framing message for
    # operator diagnosis without polluting the user-facing
    # +#message+.
    def build_wire_violation_error(message, wire_error: nil)
      Kobako::Transport::WireError.new(
        message,
        origin: Panic::ORIGIN_SANDBOX,
        klass: "Kobako::Transport::WireError",
        details: wire_error.nil? ? nil : { wire_error: wire_error }
      )
    end
  end
end
