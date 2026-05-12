# frozen_string_literal: true

module Kobako
  class Sandbox
    # Pure-function decoder for the OUTCOME_BUFFER bytes returned by
    # +__kobako_run+. Maps a tagged msgpack envelope to either the unwrapped
    # mruby return value or a raised three-layer
    # ({SPEC.md "Error Scenarios"}[link:../../../SPEC.md]) exception.
    #
    #   * tag 0x01, decode OK                 → return Result.value
    #   * tag 0x01, decode fails              → SandboxError (E-09)
    #   * tag 0x02, origin="service"          → ServiceError (E-13)
    #   * tag 0x02, origin="sandbox"/missing  → SandboxError (E-04..E-07)
    #   * tag 0x02, decode fails              → SandboxError (E-08)
    #   * unknown tag                         → TrapError    (E-03)
    module OutcomeDecoder
      module_function

      def decode(bytes)
        tag, body = split_outcome_tag(bytes)
        case tag
        when Kobako::Wire::Envelope::OUTCOME_TAG_RESULT
          decode_result(body)
        when Kobako::Wire::Envelope::OUTCOME_TAG_PANIC
          raise decode_panic(body)
        else
          raise trap_for_tag(tag)
        end
      end

      # TrapError for unknown or absent tag
      # ({SPEC.md ABI Signatures}[link:../../../SPEC.md]: len=0 and unknown-tag
      # both walk the trap path).
      def trap_for_tag(tag)
        return TrapError.new("guest exited without writing an outcome (len=0)") if tag.nil?

        TrapError.new(format("unknown outcome tag 0x%<tag>02x", tag: tag))
      end

      def split_outcome_tag(bytes)
        bytes = bytes.b
        [bytes.getbyte(0), bytes.byteslice(1, bytes.bytesize - 1)]
      end

      # Decode failure on a known Result tag is a SandboxError (E-09): the
      # framing was fine, but the wrapped value is unrepresentable.
      def decode_result(body)
        Kobako::Wire::Envelope.decode_result(body).value
      rescue Kobako::Wire::Error => e
        raise wire_violation_error("result envelope decode failed: #{e.message}")
      end

      # Decode failure on a known Panic tag is a SandboxError (E-08).
      def decode_panic(body)
        build_panic_error(Kobako::Wire::Envelope.decode_panic(body))
      rescue Kobako::Wire::Error => e
        wire_violation_error("panic envelope decode failed: #{e.message}")
      end

      # Map a decoded Panic envelope into the corresponding three-layer
      # Ruby exception. +origin == "service"+ → ServiceError (with the
      # +::Disconnected+ subclass selected when the panic carries the
      # disconnected sentinel —
      # {SPEC "Error Classes"}[link:../../../SPEC.md]); everything else
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

      # {SPEC "Error Classes"}[link:../../../SPEC.md]: when
      # +origin="service"+ and the panic +class+ field names
      # +ServiceError::Disconnected+, surface that subclass so callers can
      # rescue the disconnected path specifically (E-14).
      def panic_target_class(panic)
        return SandboxError unless panic.origin == Kobako::Wire::Envelope::Panic::ORIGIN_SERVICE

        panic.klass == "Kobako::ServiceError::Disconnected" ? ServiceError::Disconnected : ServiceError
      end

      def wire_violation_error(message)
        SandboxError.new(
          message,
          origin: Kobako::Wire::Envelope::Panic::ORIGIN_SANDBOX,
          klass: "Kobako::WireError"
        )
      end
    end
  end
end
