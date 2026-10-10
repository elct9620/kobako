# frozen_string_literal: true

require_relative "codec"
require_relative "transport/error"

module Kobako
  module Outcome # :nodoc:
    # The two +origin+ values a Panic attributes with.
    ORIGIN_SANDBOX = "sandbox"
    ORIGIN_SERVICE = "service"

    # The guest-written class names that narrow the class a Panic settles
    # as. A name absent here settles as the base class its origin already
    # chose, so the guest widens the taxonomy only by naming a class the
    # host already defines.
    SUBCLASSES = {
      "Kobako::BytecodeError" => BytecodeError,
      "Kobako::UndefinedEntrypointError" => UndefinedEntrypointError,
      "Kobako::Transport::Error" => Kobako::Transport::Error,
      "Kobako::NoServiceError" => NoServiceError,
      "Kobako::ServiceArgumentError" => ServiceArgumentError
    }.freeze
    private_constant :ORIGIN_SANDBOX, :ORIGIN_SERVICE, :SUBCLASSES

    # +panic+ is present only on the panic arm, which is what tells the
    # failure that has a record to attribute from apart from the two that
    # do not. +entrypoint+ is known to the host and never carried by the
    # wire.
    def self.reify(kind, payload, panic, entrypoint: nil)
      return decode_value(payload) if kind == :ok

      raise panic ? panic_error(panic, entrypoint) : trap_error(kind)
    end

    # +reify+ is the whole seam: one call settles one invocation, matching
    # the single entry point the Rust frontend's twin exposes. Everything
    # else here is how that decision is reached.
    class << self
      private

      # The fields land on the exception verbatim — it carries the record
      # rather than a translation of one.
      def panic_error(panic, entrypoint)
        origin, klass, message, backtrace, available = panic
        selected = error_class(origin, klass)
        attribution = { origin: origin, klass: klass, backtrace_lines: backtrace }
        return selected.new(message, **attribution) unless selected == UndefinedEntrypointError

        UndefinedEntrypointError.new(message, name: entrypoint, available: available.map(&:to_sym), **attribution)
      end

      # +origin+ picks the branch, and the guest-written class name may
      # narrow within it so callers can rescue one path specifically. A name
      # naming a class outside the branch its origin chose is ignored rather
      # than honoured: what a guest calls its exception must not move the
      # failure to a layer the attribution did not put it in.
      def error_class(origin, klass)
        base = origin == ORIGIN_SERVICE ? ServiceError : SandboxError
        selected = SUBCLASSES.fetch(klass, base)
        return base unless selected <= base

        selected
      end

      # The guest wrote nothing, or bytes the envelope cannot frame; either
      # leaves nothing to attribute to.
      def trap_error(kind)
        return TrapError.new("Sandbox exited without producing a result") if kind == :absent

        TrapError.new("Sandbox produced an unrecognised result")
      end

      # A decode fault means the framing was fine but the carried value is
      # unrepresentable.
      def decode_value(payload)
        Kobako::Codec::Decoder.decode(payload)
      rescue Kobako::Codec::Error => e
        raise wire_error("Sandbox produced an invalid result value", diagnostic: e.message)
      end

      # +klass+ names the class raised, as on every other Diagnosable.
      def wire_error(message, diagnostic: nil)
        Kobako::Transport::Error.new(
          message,
          origin: ORIGIN_SANDBOX,
          klass: "Kobako::Transport::Error",
          diagnostic: diagnostic
        )
      end
    end
  end
end
