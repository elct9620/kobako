# frozen_string_literal: true

require_relative "codec"
require_relative "transport/error"

module Kobako
  # Host-facing boundary for the invocation outcome the native side split
  # off the core envelope. Takes the arm it named plus the fields that arm
  # carries, and settles the invocation the way a host does: return the
  # value, or raise the exception the failure attributes to.
  #
  # This is the two-step attribution decision. The wire framing belongs to
  # the native side; the payload adapter at +Kobako::Codec+ decodes only
  # what an arm carries.
  module Outcome
    # The two +origin+ values a Panic attributes with.
    ORIGIN_SANDBOX = "sandbox"
    ORIGIN_SERVICE = "service"

    module_function

    # Settle one invocation. +kind+ names the arm, +payload+ is its
    # adapter-encoded content (the value on +:result+, the Panic's details
    # on +:panic+), and +panic+ carries the attribution fields
    # +[origin, class, message, backtrace]+ — present on the panic arm and
    # absent on every other, which is what tells the failure that has a
    # record to attribute from apart from the two that do not.
    def reify(kind, payload, panic)
      return decode_value(payload) if kind == :result

      raise panic ? panic_error(payload, panic) : trap_error(kind)
    end

    # Map a Panic's attribution fields onto the three-layer taxonomy. The
    # fields land on the exception verbatim — it carries the record rather
    # than a translation of one.
    def panic_error(details, panic)
      origin, klass, message, backtrace = panic
      error_class(origin, klass).new(
        message, origin: origin, klass: klass,
                 backtrace_lines: backtrace, details: decode_details(details)
      )
    end

    # +origin == "service"+ selects ServiceError; a sandbox-origin failure
    # carrying the bytecode rejection class selects the BytecodeError
    # subclass so callers can rescue that path specifically.
    def error_class(origin, klass)
      return ServiceError if origin == ORIGIN_SERVICE

      klass == "Kobako::BytecodeError" ? BytecodeError : SandboxError
    end

    # An arm the host cannot settle: the guest wrote nothing, or wrote
    # bytes the envelope cannot frame. Either leaves nothing to attribute
    # to, so both walk the trap path; only the absent-versus-present
    # distinction selects the message.
    def trap_error(kind)
      return TrapError.new("Sandbox exited without producing a result") if kind == :absent

      TrapError.new(
        "Sandbox produced an unrecognised result; the runtime is corrupted, " \
        "discard this Sandbox before another invocation"
      )
    end

    # The Result arm's value. A decode fault means the framing was fine
    # but the carried value is unrepresentable; the specific codec fault
    # is stashed in +details+ rather than spliced into the message —
    # callers cannot act on the inner "Symbol payload must be …" wording,
    # but operators triaging a corrupted Sandbox runtime still need it.
    def decode_value(payload)
      # A Result is a payload position: an ext 0x02 Fault in it is a wire
      # violation, since a Fault's only home is a Reply's fault arm.
      Kobako::Codec.forbid_faults { Kobako::Codec::Decoder.decode(payload) }
    rescue Kobako::Codec::Error => e
      raise wire_error("Sandbox produced an invalid result value", detail: e.message)
    end

    # A Panic's structured diagnostics, or +nil+ when the arm carried
    # none. Details are a payload position, so an ext 0x02 Fault in them
    # is a wire violation — a Panic whose diagnostics violate the wire is
    # not a record worth attributing from, and the invalid-record channel
    # takes it instead.
    def decode_details(payload)
      return nil if payload.empty?

      Kobako::Codec.forbid_faults { Kobako::Codec::Decoder.decode(payload) }
    rescue Kobako::Codec::Error => e
      raise wire_error("Sandbox produced an invalid panic record", detail: e.message)
    end

    # Lift a wire violation the host detected to the real
    # +Kobako::Transport::Error+ class so callers can +rescue+ it
    # specifically instead of pattern-matching on +error.klass+. The
    # +klass+ field is still populated so existing operator-side tooling
    # that greps on the string continues to work. +detail+ carries the
    # inner codec message for operator diagnosis without polluting the
    # user-facing +#message+.
    def wire_error(message, detail: nil)
      Kobako::Transport::Error.new(
        message,
        origin: ORIGIN_SANDBOX,
        klass: "Kobako::Transport::Error",
        details: detail
      )
    end
  end
end
