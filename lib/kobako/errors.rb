# frozen_string_literal: true

# Top-level Kobako namespace.
module Kobako
  # Three-class error taxonomy (docs/behavior.md § Error Scenarios).
  #
  # Every `Kobako::Sandbox` invocation (`#eval` or `#run`) either returns a value or raises
  # exactly one of these three classes. Attribution is decided after the
  # guest binary returns control to the host (docs/behavior.md
  # "Step 1 — Wasm trap" then "Step 2 — Outcome envelope tag").
  #
  # Three top-level branches:
  #
  #   * {TrapError}     — Wasm engine layer (trap, OOM, unreachable, or a
  #                       wire-violation fallback signalling a corrupted
  #                       guest runtime).
  #   * {SandboxError}  — sandbox / wire layer (mruby script error,
  #                       wire-decode failure on an otherwise valid tag,
  #                       Catalog::Handler exhaustion, output buffer overrun).
  #   * {ServiceError}  — service / capability layer (a Service capability
  #                       call that failed and was not rescued inside the
  #                       script).
  #
  # Subclasses pinned by docs/behavior.md Error Classes:
  #
  #   * {HandlerExhaustedError} < {SandboxError} — Handle id cap hit (B-21).
  #   * {ServiceError::Disconnected} < {ServiceError} — `:disconnected`
  #                       sentinel hit on the Catalog::Handler (E-14).

  # Base for all kobako-raised errors so callers that want to ignore the
  # taxonomy can rescue a single class.
  class Error < StandardError; end

  # Wasm engine layer. Raised when the Wasm execution engine crashed
  # (trap, OOM, unreachable) or when the wire layer detected a structural
  # violation that signals a corrupted guest execution environment
  # (zero-length OUTCOME_BUFFER, unknown outcome tag — SPEC E-02 / E-03).
  #
  # Two named subclasses cover the configured per-invocation caps from B-01:
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

  # Wall-clock timeout cap exhausted. {docs/behavior.md E-19}[link:../../docs/behavior.md]:
  # the absolute deadline +entry_time + timeout+ passed and the next guest
  # wasm safepoint trapped. The Sandbox is unrecoverable after this point;
  # discard and recreate before another execution.
  class TimeoutError < TrapError; end

  # Linear-memory cap exhausted. {docs/behavior.md E-20}[link:../../docs/behavior.md]:
  # a guest +memory.grow+ would have pushed linear memory past the
  # configured +memory_limit+. The Sandbox is unrecoverable after this
  # point; discard and recreate before another execution.
  class MemoryLimitError < TrapError; end

  # Setup-time error raised by +Kobako::Sandbox.new+ /
  # +Kobako::Runtime.from_path+ when +data/kobako.wasm+ is missing —
  # the common pre-build state on a fresh clone before
  # +bundle exec rake compile+. Not a runtime trap (the wasm engine
  # never started), so it sits directly under +Kobako::Error+ rather
  # than under +TrapError+.
  class ModuleNotBuiltError < Error; end

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

    # docs/behavior.md Error Classes: ServiceError::Disconnected is raised
    # when the dispatch target Handle resolves to the `:disconnected`
    # sentinel in the Catalog::Handler (ABA protection rule — id exists
    # but entry was invalidated). E-14.
    class Disconnected < ServiceError; end
  end

  # docs/behavior.md Error Classes: HandlerExhaustedError is the canonical
  # SandboxError subclass for the id-cap-hit path (B-21). Raised when the
  # per-invocation Handle ID counter in Catalog::Handler reaches
  # +0x7fff_ffff+ (2³¹ − 1) and further allocation would exceed the cap.
  class HandlerExhaustedError < SandboxError; end

  # docs/behavior.md Error Classes: BytecodeError is the SandboxError
  # subclass raised when a `#preload(binary:)` snippet fails structural
  # validation during the first invocation's snippet replay against a
  # fresh `mrb_state` (E-37 RITE version mismatch, E-38 corrupt body).
  # Bytecode that loads cleanly and then raises at top level is E-36
  # and surfaces as plain `SandboxError` with the natural mruby class
  # preserved. Inherits from SandboxError so a single
  # `rescue Kobako::SandboxError` covers both source and bytecode
  # snippet failures while callers wanting bytecode-specific handling
  # can `rescue Kobako::BytecodeError` directly.
  class BytecodeError < SandboxError; end
end
