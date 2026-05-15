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
  #
  # Two named subclasses cover the configured per-run caps from B-01:
  #
  #   * {TimeoutError}     — wall-clock +timeout+ exceeded (E-19).
  #   * {MemoryLimitError} — guest +memory.grow+ would exceed
  #                          +memory_limit+ (E-20).
  #
  # Host Apps that only care about "guest is unrecoverable, discard the
  # Sandbox" can rescue +TrapError+ and ignore the subclass; Host Apps that
  # want to surface a specific reason to operators can rescue the subclass
  # first.
  class TrapError < Error; end

  # Wall-clock timeout cap exhausted. {SPEC.md E-19}[link:../../SPEC.md]:
  # the absolute deadline +entry_time + timeout+ passed and the next guest
  # wasm safepoint trapped. The Sandbox is unrecoverable after this point;
  # discard and recreate before another execution.
  class TimeoutError < TrapError; end

  # Linear-memory cap exhausted. {SPEC.md E-20}[link:../../SPEC.md]:
  # a guest +memory.grow+ would have pushed linear memory past the
  # configured +memory_limit+. The Sandbox is unrecoverable after this
  # point; discard and recreate before another execution.
  class MemoryLimitError < TrapError; end

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
