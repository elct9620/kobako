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
  # the native side; the payload codec at +Kobako::Codec+ decodes only
  # the one arm that carries a value.
  module Outcome
    # The two +origin+ values a Panic attributes with.
    ORIGIN_SANDBOX = "sandbox"
    ORIGIN_SERVICE = "service"

    # The guest-written class names that select a +SandboxError+ subclass.
    # A name absent here settles as plain +SandboxError+, so the guest
    # widens the taxonomy only by naming a class the host already defines.
    SUBCLASSES = {
      "Kobako::BytecodeError" => BytecodeError,
      "Kobako::UndefinedEntrypointError" => UndefinedEntrypointError
    }.freeze

    module_function

    # Settle one invocation. +kind+ names the arm, +payload+ is the
    # codec-encoded value the +:result+ arm carries, and +panic+ carries
    # the Panic's fields +[origin, class, message, backtrace, available]+ —
    # present on the panic arm and absent on every other, which is what
    # tells the failure that has a record to attribute from apart from the
    # two that do not. +entrypoint+ is the name this invocation asked for,
    # which the host knows and the wire therefore never carries.
    def reify(kind, payload, panic, entrypoint: nil)
      return decode_value(payload) if kind == :result

      raise panic ? panic_error(panic, entrypoint) : trap_error(kind)
    end

    # Map a Panic's fields onto the three-layer taxonomy. The fields land
    # on the exception verbatim — it carries the record rather than a
    # translation of one.
    def panic_error(panic, entrypoint)
      origin, klass, message, backtrace, available = panic
      selected = error_class(origin, klass)
      attribution = { origin: origin, klass: klass, backtrace_lines: backtrace }
      return selected.new(message, **attribution) unless selected == UndefinedEntrypointError

      UndefinedEntrypointError.new(message, name: entrypoint, available: available.map(&:to_sym), **attribution)
    end

    # +origin == "service"+ selects ServiceError; a sandbox-origin failure
    # naming one of the guest-written subclass names selects that subclass
    # so callers can rescue that path specifically.
    def error_class(origin, klass)
      return ServiceError if origin == ORIGIN_SERVICE

      SUBCLASSES.fetch(klass, SandboxError)
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

    # The Result arm's value — the one position a payload codec still
    # owns on this path. A decode fault means the framing was fine but the
    # carried value is unrepresentable.
    def decode_value(payload)
      # A Result is a payload position: an ext 0x02 Fault in it is a wire
      # violation, since a Fault's only home is a Reply's fault arm.
      Kobako::Codec.forbid_faults { Kobako::Codec::Decoder.decode(payload) }
    rescue Kobako::Codec::Error => e
      raise wire_error("Sandbox produced an invalid result value", diagnostic: e.message)
    end

    # Lift a wire violation the host detected to the real
    # +Kobako::Transport::Error+ class so callers can +rescue+ it
    # specifically instead of pattern-matching on +error.klass+. The
    # +klass+ field is still populated so existing operator-side tooling
    # that greps on the string continues to work.
    def wire_error(message, diagnostic: nil)
      Kobako::Transport::Error.new(
        message,
        origin: ORIGIN_SANDBOX,
        klass: "Kobako::Transport::Error",
        diagnostic: diagnostic
      )
    end

    # +reify+ is the whole seam: one call settles one invocation, matching
    # the single entry point the Rust frontend's twin exposes. Everything
    # else here is how that decision is reached.
    private_class_method :panic_error, :error_class, :trap_error, :decode_value, :wire_error
    private_constant :ORIGIN_SANDBOX, :ORIGIN_SERVICE, :SUBCLASSES
  end
end
