# frozen_string_literal: true

module Kobako
  # Every Sandbox#eval or Sandbox#run returns, or raises exactly one of
  # TrapError, SandboxError, or ServiceError. SetupError and PoolTimeoutError
  # arise outside any invocation.

  # The base of every error kobako raises.
  class Error < StandardError; end

  # Gives an invocation failure the Execution of the run that failed, so a
  # rescue reads its captures and usage as a successful caller would.
  # +execution+ is +nil+ when the failure came before the guest ran.
  module CarriesExecution
    # The Execution of the run that failed.
    attr_reader :execution

    def with_execution(execution) # :nodoc:
      @execution = execution
      self
    end
  end

  # The engine stopped the guest: a trap, a cap reached, or an outcome that
  # could not be read. The next invocation on the same Sandbox runs as usual.
  class TrapError < Error
    include CarriesExecution
  end

  # The invocation ran past its +timeout+.
  class TimeoutError < TrapError; end

  # The invocation grew guest memory past its +memory_limit+.
  class MemoryLimitError < TrapError; end

  # A Sandbox could not be built: the Guest Binary is unreadable or not a
  # valid module, the runtime failed to start, or it provides less isolation
  # than +profile+ asks for.
  class SetupError < Error; end

  # The Guest Binary is missing at +wasm_path+.
  class ModuleNotBuiltError < SetupError; end

  # What the guest reported about a failure.
  module Diagnosable
    # Where the failure came from (<tt>"sandbox"</tt> or <tt>"service"</tt>),
    # the name of the class that raised it, and its backtrace.
    attr_reader :origin, :klass, :backtrace_lines

    def initialize(message, origin: nil, klass: nil, backtrace_lines: nil) # :nodoc:
      super(message)
      @origin = origin
      @klass = klass
      @backtrace_lines = backtrace_lines
    end
  end

  # The guest's own code failed, or what it produced could not be read.
  class SandboxError < Error
    include Diagnosable
    include CarriesExecution
  end

  # A Service call failed and the guest left it unrescued.
  class ServiceError < Error
    include Diagnosable
    include CarriesExecution
  end

  # The call reached no Service method: nothing is bound there, the Handle
  # is not live, or the guest may not call that method. The causes look the
  # same on purpose, so a target discloses nothing about what it defines.
  class NoServiceError < ServiceError; end

  # The call reached the Service method with arguments that did not fit.
  class ServiceArgumentError < ServiceError; end

  # Raised at a Service's +yield+ when the guest block raised; rescue it as
  # you would the block's own exception. Left unrescued, the guest sees its
  # own exception again, so it never reaches the Host App.
  class BlockError < Error
    include Diagnosable
  end

  # Raised at a Service's +yield+ when an argument cannot cross to the guest,
  # so the block never ran. Left unrescued, the Service call fails.
  class YieldValueError < Error; end

  # One invocation issued more Handles than an id can number (2**31 - 1).
  class HandleExhaustedError < SandboxError; end

  # A <tt>preload(binary:)</tt> snippet failed to load. Bytecode that loads
  # and then raises is a plain SandboxError.
  class BytecodeError < SandboxError; end

  # Sandbox#run named no top-level constant.
  class UndefinedEntrypointError < SandboxError
    # The constant asked for, and the ones the preloaded snippets define.
    attr_reader :name, :available

    def initialize(message, name: nil, available: [], **) # :nodoc:
      super(message, **)
      @name = name
      @available = available
    end
  end

  # Pool#with waited past +checkout_timeout+ with every slot held. Retrying
  # succeeds once a holder returns its Sandbox.
  class PoolTimeoutError < Error; end
end
