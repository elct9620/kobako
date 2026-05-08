# frozen_string_literal: true

require_relative "errors"
require_relative "wire/envelope"
require_relative "wire/error"

module Kobako
  # OutcomeAttribution — pure mapping from raw OUTCOME_BUFFER bytes to
  # the three-class kobako error taxonomy (SPEC §"Error Scenarios").
  #
  # Drives Step 2 of the two-step decision tree (SPEC §"Error Scenarios"
  # at lines 463–481). Step 1 (wasmtime trap) is the responsibility of
  # the caller: any wasmtime-side error must be raised as TrapError
  # before the OUTCOME_BUFFER bytes are read.
  #
  # Attribution table (SPEC §"Error Scenarios"):
  #
  #   * tag 0x01, decode OK                 → return Result.value
  #   * tag 0x01, decode fails              → SandboxError (E-09)
  #   * tag 0x02, origin="service"          → ServiceError (E-13)
  #   * tag 0x02, origin="sandbox"/missing  → SandboxError (E-04..E-07)
  #   * tag 0x02, decode fails              → SandboxError (E-08)
  #   * unknown tag                         → TrapError    (E-03)
  #
  # The unknown-tag branch is the wire-violation fallback (sharing the
  # Trap path with Step 1); a known tag with a malformed payload is
  # attributed to the sandbox layer because the envelope framing was
  # unambiguous.
  module OutcomeAttribution
    module_function

    # Decode +bytes+ and either return the wrapped value (Result envelope)
    # or raise a SandboxError / ServiceError / TrapError per the table
    # above. The caller pre-validates that +bytes+ is non-empty (zero-length
    # OUTCOME_BUFFER is SPEC E-02 → TrapError, raised by the caller).
    def decode(bytes)
      tag, body = split_tag(bytes)
      case tag
      when Kobako::Wire::Envelope::OUTCOME_TAG_RESULT
        decode_result(body)
      when Kobako::Wire::Envelope::OUTCOME_TAG_PANIC
        raise decode_panic(body)
      else
        raise TrapError, format("unknown outcome tag 0x%<tag>02x", tag: tag)
      end
    end

    def split_tag(bytes)
      bytes = bytes.b
      [bytes.getbyte(0), bytes.byteslice(1, bytes.bytesize - 1)]
    end

    # Decode failure on a known Result tag is a SandboxError (E-09): the
    # framing was fine, but the wrapped value is unrepresentable.
    def decode_result(body)
      Kobako::Wire::Envelope.decode_result(body).value
    rescue Kobako::Wire::Error => e
      raise wire_violation_error(SandboxError, "result envelope decode failed: #{e.message}")
    end

    # Decode failure on a known Panic tag is a SandboxError (E-08); on
    # success, attribution falls to {#build_panic_error} (origin-based).
    def decode_panic(body)
      build_panic_error(Kobako::Wire::Envelope.decode_panic(body))
    rescue Kobako::Wire::Error => e
      wire_violation_error(SandboxError, "panic envelope decode failed: #{e.message}")
    end

    # Map a decoded Panic envelope into the corresponding three-layer
    # Ruby exception. `origin == "service"` → ServiceError; everything
    # else → SandboxError.
    def build_panic_error(panic)
      target_class = panic.origin == Kobako::Wire::Envelope::Panic::ORIGIN_SERVICE ? ServiceError : SandboxError
      target_class.new(
        panic.message,
        origin: panic.origin,
        klass: panic.klass,
        backtrace_lines: panic.backtrace,
        details: panic.details
      )
    end

    def wire_violation_error(klass, message)
      klass.new(
        message,
        origin: Kobako::Wire::Envelope::Panic::ORIGIN_SANDBOX,
        klass: "Kobako::WireError"
      )
    end
  end
end
