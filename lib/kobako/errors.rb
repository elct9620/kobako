# frozen_string_literal: true

# Top-level Kobako namespace.
module Kobako
  # Error taxonomy.
  #
  # Every +Kobako::Sandbox+ invocation (+#eval+ or +#run+) either returns a value or raises
  # exactly one of three invocation-outcome classes. Attribution is decided after the
  # guest binary returns control to the host: first the Wasm-trap layer, then
  # the outcome-envelope tag.
  #
  # Three invocation-outcome branches:
  #
  #   * TrapError     — Wasm engine layer (trap, OOM, unreachable, or a
  #                       wire-violation fallback signalling a corrupted
  #                       guest runtime).
  #   * SandboxError  — sandbox / wire layer (mruby script error,
  #                       wire-decode failure on an otherwise valid tag,
  #                       Catalog::Handles exhaustion, output buffer overrun).
  #   * ServiceError  — service / capability layer (a Service capability
  #                       call that failed and was not rescued inside the
  #                       script).
  #
  # Two further branches sit outside the invocation taxonomy:
  #
  #   * SetupError    — construction layer. Raised by +Kobako::Sandbox.new+
  #                       when the wasm runtime cannot be built from the
  #                       configured +wasm_path+ before any invocation runs.
  #                       Not an invocation outcome, so it never passes
  #                       through the two-step attribution decision.
  #   * PoolTimeoutError — pool checkout layer. Raised by +Kobako::Pool#with+
  #                       when the checkout wait exceeds +checkout_timeout+.
  #
  # Named subclasses:
  #
  #   * ModuleNotBuiltError < SetupError — Guest Binary artifact absent
  #                       at +wasm_path+.
  #   * HandleExhaustedError < SandboxError — Handle id cap hit.

  # Base for all kobako-raised errors so callers that want to ignore the
  # taxonomy can rescue a single class.
  class Error < StandardError; end

  # Wasm engine layer. Raised when the Wasm execution engine crashed
  # (trap, OOM, unreachable) or when the wire layer detected a structural
  # violation that signals a corrupted guest execution environment
  # (zero-length OUTCOME_BUFFER, unknown outcome tag).
  #
  # Two named subclasses cover the configured per-invocation caps:
  #
  #   * TimeoutError     — wall-clock +timeout+ exceeded.
  #   * MemoryLimitError — guest +memory.grow+ would exceed
  #                          +memory_limit+.
  #
  # Host Apps that only care about "guest is unrecoverable, discard the
  # Sandbox" can rescue +TrapError+ and ignore the subclass; Host Apps that
  # want to surface a specific reason to operators can rescue the subclass
  # first.
  class TrapError < Error; end

  # Wall-clock timeout cap exhausted: the absolute deadline
  # +entry_time + timeout+ passed and the next guest wasm safepoint
  # trapped. The Sandbox is unrecoverable after this point; discard and
  # recreate before another execution.
  class TimeoutError < TrapError; end

  # Linear-memory cap exhausted: a guest +memory.grow+ would have pushed
  # linear memory past the configured +memory_limit+. The Sandbox is
  # unrecoverable after this point; discard and recreate before another
  # execution.
  class MemoryLimitError < TrapError; end

  # Construction-layer error raised by +Kobako::Sandbox.new+ /
  # +Kobako::Runtime.from_path+ when the wasm runtime cannot be built
  # from the configured +wasm_path+ before any invocation runs —
  # an unreadable artifact, bytes that are not a valid Wasm module, or
  # engine / linker / instantiation setup failure. Construction
  # is not an invocation, so +SetupError+ sits beside the invocation
  # taxonomy under +Kobako::Error+ rather than under +TrapError+: no
  # Sandbox is produced, so the +TrapError+ "discard and recreate"
  # recovery contract does not apply.
  class SetupError < Error; end

  # The named +SetupError+ subclass for the common, actionable case:
  # the Guest Binary artifact is absent at +wasm_path+ — the pre-build
  # state on a fresh clone before +bundle exec rake compile+. Host Apps
  # that only need "the Sandbox could not be set up" rescue +SetupError+;
  # those wanting to special-case the unbuilt-artifact state rescue
  # +ModuleNotBuiltError+ first.
  class ModuleNotBuiltError < SetupError; end

  # The structured attribution the two invocation-failure classes carry
  # from a decoded guest exception — its +origin+, original +klass+,
  # +backtrace_lines+, and +details+ — so a Host App can inspect a failure
  # beyond its message. Mixed into both rather than promoted to a shared
  # superclass because +SandboxError+ and +ServiceError+ sit in distinct
  # branches of the invocation-outcome taxonomy under +Kobako::Error+.
  module StructuredError
    attr_reader :origin, :klass, :backtrace_lines, :details

    def initialize(message, origin: nil, klass: nil, backtrace_lines: nil, details: nil)
      super(message)
      @origin = origin
      @klass = klass
      @backtrace_lines = backtrace_lines
      @details = details
    end
  end

  # Sandbox / wire layer. Raised when the guest ran to completion but
  # execution failed due to a mruby script error, a protocol fault, or a
  # host-side wire decode failure on an otherwise valid outcome tag.
  class SandboxError < Error
    include StructuredError
  end

  # Service layer. Raised when a Service capability call inside a mruby
  # script reported an application-level failure that the script did not
  # rescue.
  class ServiceError < Error
    include StructuredError
  end

  # HandleExhaustedError is the canonical SandboxError subclass for the
  # id-cap-hit path. Raised when the per-invocation Handle ID counter in
  # Catalog::Handles reaches +0x7fff_ffff+ (2³¹ − 1) and further
  # allocation would exceed the cap.
  class HandleExhaustedError < SandboxError; end

  # BytecodeError is the SandboxError subclass raised when a
  # +#preload(binary:)+ snippet fails structural validation during the
  # first invocation's snippet replay against a fresh +mrb_state+ (RITE
  # version mismatch or corrupt body). Bytecode that loads cleanly and
  # then raises at top level surfaces as plain +SandboxError+ with the
  # natural mruby class preserved. Inherits from SandboxError so a single
  # +rescue Kobako::SandboxError+ covers both source and bytecode
  # snippet failures while callers wanting bytecode-specific handling
  # can +rescue Kobako::BytecodeError+ directly.
  class BytecodeError < SandboxError; end

  # Pool checkout layer. Raised by +Kobako::Pool#with+ when the checkout
  # wait exceeded the configured +checkout_timeout+ while every slot was
  # held. No Sandbox state is touched — retrying succeeds as soon as a holder
  # returns its Sandbox.
  class PoolTimeoutError < Error; end
end
