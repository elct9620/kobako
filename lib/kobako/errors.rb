# frozen_string_literal: true

# Top-level Kobako namespace.
module Kobako
  # Three-class error taxonomy (SPEC.md → Error Scenarios).
  #
  # Every `Kobako::Sandbox#run` invocation either returns a value or raises
  # exactly one of these three classes. Attribution is decided after the
  # guest binary returns control to the host (SPEC "Step 1 — Wasm trap"
  # then "Step 2 — Outcome envelope tag").
  #
  # Three top-level branches:
  #
  #   * {TrapError}     — Wasm engine layer (trap, OOM, unreachable, or a
  #                       wire-violation fallback signalling a corrupted
  #                       guest runtime).
  #   * {SandboxError}  — sandbox / wire layer (mruby script error,
  #                       wire-decode failure on an otherwise valid tag,
  #                       HandleTable exhaustion, output buffer overrun).
  #   * {ServiceError}  — service / capability layer (a Service RPC that
  #                       failed and was not rescued inside the script).
  #
  # Subclasses pinned by SPEC "Error Classes":
  #
  #   * {HandleTableExhausted} < {SandboxError}    — id cap hit (B-21).
  #   * {ServiceError::Disconnected} < {ServiceError} — `:disconnected`
  #                       sentinel hit on the HandleTable (E-14).

  # Base for all kobako-raised errors so callers that want to ignore the
  # taxonomy can rescue a single class.
  class Error < StandardError; end

  # Wasm engine layer. Raised when the Wasm execution engine crashed
  # (trap, OOM, unreachable) or when the wire layer detected a structural
  # violation that signals a corrupted guest execution environment
  # (zero-length OUTCOME_BUFFER, unknown outcome tag — SPEC E-02 / E-03).
  class TrapError < Error; end

  # Sandbox / wire layer. Raised when the guest ran to completion but
  # execution failed due to a mruby script error, a protocol fault, or a
  # host-side wire decode failure on an otherwise valid outcome tag.
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

    # SPEC "Error Classes": ServiceError::Disconnected is raised
    # when the RPC target Handle resolves to the `:disconnected` sentinel
    # in the HandleTable (ABA protection rule — id exists but entry was
    # invalidated). E-14.
    class Disconnected < ServiceError; end
  end

  # HandleTable lookup-failure error (unknown id passed to #fetch /
  # #release). A SandboxError subclass: per the wire-layer rule, an
  # unknown Handle id surfaces as a `type="undefined"` Response.err
  # envelope inside RpcDispatcher and never reaches the Host App
  # directly; outside that path (e.g. tests poking the HandleTable
  # directly), it surfaces as a SandboxError.
  class HandleTableError < SandboxError; end

  # SPEC "Error Classes": HandleTableExhausted is the canonical
  # SandboxError subclass for the id-cap-hit path (B-21). Inherits from
  # HandleTableError so a single `rescue Kobako::HandleTableError` covers
  # both lookup-failure and cap-exhaustion paths.
  class HandleTableExhausted < HandleTableError; end
end
