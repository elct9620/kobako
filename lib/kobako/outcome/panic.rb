# frozen_string_literal: true

module Kobako
  module Outcome
    # SPEC.md → Outcome Envelope → Panic envelope ({SPEC.md Outcome
    # Envelope}[link:../../../SPEC.md]). Wire-shaped failure record
    # carried in the OUTCOME_BUFFER when the guest run terminates with
    # an uncaught top-level exception.
    #
    # This is the **wire data**, not a raisable Ruby exception. The
    # mapping from Panic to a three-layer Ruby exception (TrapError /
    # SandboxError / ServiceError) happens at +Kobako::Outcome.decode+
    # via +build_panic_error+ — callers never raise +Panic+ directly.
    #
    # The five fields mirror SPEC: +origin+ ("sandbox" / "service"),
    # +klass+ (the guest-side exception class name as a String),
    # +message+, +backtrace+ (Array of String), +details+ (any
    # wire-legal value, nil when absent). Required-field validation is
    # enforced at construction; the +ORIGIN_SANDBOX+ / +ORIGIN_SERVICE+
    # constants pin the two SPEC-defined origin values.
    #
    # Built on the +class X < Data.define(...)+ subclass form so the
    # class body is fully Steep-visible; ruby/rbs upstream documents
    # this as the Steep-friendly shape and the +Style/DataInheritance+
    # cop is disabled on that basis (see +.rubocop.yml+).
    class Panic < Data.define(:origin, :klass, :message, :backtrace, :details)
      ORIGIN_SANDBOX = "sandbox"
      ORIGIN_SERVICE = "service"

      def initialize(origin:, klass:, message:, backtrace: [], details: nil)
        raise ArgumentError, "Panic origin must be String"  unless origin.is_a?(String)
        raise ArgumentError, "Panic class must be String"   unless klass.is_a?(String)
        raise ArgumentError, "Panic message must be String" unless message.is_a?(String)
        unless backtrace.is_a?(Array) && backtrace.all?(String)
          raise ArgumentError, "Panic backtrace must be Array of String"
        end

        super
      end
    end
  end
end
