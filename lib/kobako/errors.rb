# frozen_string_literal: true

# Top-level Kobako namespace.
module Kobako
  # Error taxonomy.
  #
  # Every +Kobako::Sandbox+ invocation (+#eval+ or +#run+) either returns a
  # value or raises exactly one of TrapError, SandboxError, or ServiceError.
  # Attribution is decided after the guest binary returns control to the
  # host: first the Wasm-trap layer, then the outcome-envelope tag.
  # SetupError and PoolTimeoutError sit outside the invocation taxonomy and
  # never pass through that attribution decision. Each class below
  # documents its own layer.

  # Base for all kobako-raised errors so callers that want to ignore the
  # taxonomy can rescue a single class.
  class Error < StandardError; end

  # Carries the frozen +Kobako::Execution+ of the run that failed, so a
  # rescue reads its captures and usage exactly as a successful caller reads
  # them off the return value. Mixed only into the three invocation-outcome
  # classes (+TrapError+ / +SandboxError+ / +ServiceError+ and their
  # subclasses); +SetupError+ and +PoolTimeoutError+ arise outside any
  # invocation and carry no Execution. Defaults to +nil+ — a pre-flight
  # failure that ran no invocation leaves it unset.
  module CarriesExecution
    attr_reader :execution

    # Attach the failed run's +Kobako::Execution+ and return +self+ so the
    # raise site reads +raise error.with_execution(execution)+.
    def with_execution(execution)
      @execution = execution
      self
    end
  end

  # Wasm engine layer. Raised when the Wasm execution engine crashed
  # (trap, OOM, unreachable) or when the wire layer detected a structural
  # violation that signals a corrupted guest execution environment (an
  # outcome the core envelope cannot frame, an absent one included).
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
  class TrapError < Error
    include CarriesExecution
  end

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
  # from a decoded guest exception — its +origin+, original +klass+, and
  # +backtrace_lines+ — so a Host App can inspect a failure beyond its
  # message. Mixed into both rather than promoted to a shared superclass
  # because +SandboxError+ and +ServiceError+ sit in distinct branches of
  # the invocation-outcome taxonomy under +Kobako::Error+.
  #
  # Data specific to one kind of failure rides a named reader on the
  # subclass that failure raises, the way Ruby pairs NameError#name with
  # +NameError#local_variables+ — see UndefinedEntrypointError.
  module Diagnosable
    attr_reader :origin, :klass, :backtrace_lines

    def initialize(message, origin: nil, klass: nil, backtrace_lines: nil)
      super(message)
      @origin = origin
      @klass = klass
      @backtrace_lines = backtrace_lines
    end
  end

  # Sandbox / wire layer. Raised when the guest ran to completion but
  # execution failed due to a mruby script error, a protocol fault, or a
  # payload the host's codec could not decode out of a well-framed
  # outcome.
  class SandboxError < Error
    include Diagnosable
    include CarriesExecution
  end

  # Service layer. Raised when a Service capability call inside a mruby
  # script reported an application-level failure that the script did not
  # rescue. The base class covers a Service that ran and raised; the two
  # subclasses below cover the calls that never reached one, so a Host App
  # routes them apart with +rescue+ instead of by reading the message.
  class ServiceError < Error
    include Diagnosable
    include CarriesExecution
  end

  # The ServiceError subclass raised when the call reached no Service
  # method: the bound path holds nothing, the Capability Handle is not
  # live in this invocation, or the method is absent or outside the guest
  # surface. The causes stay indistinguishable — an opaque target must
  # disclose nothing about which methods it defines — so what a Host App
  # learns is that this call will not succeed by being retried.
  class NoServiceError < ServiceError; end

  # The ServiceError subclass raised when the call reached the Service
  # method but its arguments did not fit — an unknown keyword, or an
  # arity mismatch.
  class ServiceArgumentError < ServiceError; end

  # Raised at a Service method's +yield+ site when the guest block it
  # yielded to raised. A Service rescues it the way it would rescue a
  # block's exception without a Sandbox between the two frames; leaving it
  # unrescued returns it to the guest, which re-raises the exception it
  # raised in the first place. It therefore never reaches the Host App as
  # an invocation outcome and carries no Execution. +klass+ names the
  # guest-side class, which has no host counterpart to rebuild.
  class BlockError < Error
    include Diagnosable
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

  # UndefinedEntrypointError is the SandboxError subclass raised when a
  # +#run+ target names no top-level constant in the guest. It carries the
  # +#name+ that was asked for and the +#available+ names it could have
  # been — the top-level constants the preloaded snippets contributed — so
  # a caller corrects the name from the error rather than by reading the
  # guest source. Ruby pairs NameError#name with
  # +NameError#local_variables+ for the same reason.
  class UndefinedEntrypointError < SandboxError
    attr_reader :name, :available

    def initialize(message, name: nil, available: [], **)
      super(message, **)
      @name = name
      @available = available
    end
  end

  # Pool checkout layer. Raised by +Kobako::Pool#with+ when the checkout
  # wait exceeded the configured +checkout_timeout+ while every slot was
  # held. No Sandbox state is touched — retrying succeeds as soon as a holder
  # returns its Sandbox.
  class PoolTimeoutError < Error; end
end
