# frozen_string_literal: true

module Kobako
  # Three-class error taxonomy (SPEC.md → Error Scenarios).
  #
  # Every `Kobako::Sandbox#run` invocation either returns a value or raises
  # exactly one of these three classes. Attribution is decided after the
  # guest binary returns control to the host (SPEC §"Step 1 — Wasm trap"
  # then §"Step 2 — Outcome envelope tag").
  #
  # Item #20 will flesh out the public surface of these classes
  # (rich attributes — `class`, `origin`, `backtrace`, `details` — plus
  # the canonical `Kobako::HandleTableExhausted < SandboxError` subclass).
  # For #16 we only need them to (a) exist as distinct classes the Host
  # App can rescue separately and (b) carry a message for debugging.

  # Base for all kobako-raised errors so callers that want to ignore the
  # taxonomy can rescue a single class.
  class Error < StandardError; end

  # Wasm engine layer. Raised when the Wasm execution engine crashed
  # (trap, OOM, unreachable) or when the wire layer detected a structural
  # violation that signals a corrupted guest execution environment.
  class TrapError < Error; end

  # Sandbox / wire layer. Raised when the guest ran to completion but
  # execution failed due to a mruby script error, protocol fault, or a
  # host-side wire decode failure.
  class SandboxError < Error
    attr_reader :origin, :klass, :backtrace_lines, :details

    def initialize(message, origin: nil, klass: nil, backtrace_lines: nil, details: nil)
      super(message)
      @origin = origin
      @klass = klass
      @backtrace_lines = backtrace_lines
      @details = details
    end
  end

  # Service layer. Raised when a Service capability call inside a mruby
  # script reported an application-level failure that the script did not
  # rescue.
  class ServiceError < Error
    attr_reader :origin, :klass, :backtrace_lines, :details

    def initialize(message, origin: nil, klass: nil, backtrace_lines: nil, details: nil)
      super(message)
      @origin = origin
      @klass = klass
      @backtrace_lines = backtrace_lines
      @details = details
    end
  end
end
