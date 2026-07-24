# frozen_string_literal: true

module Parity
  # Interprets a Scenario against +Kobako::Sandbox+ and emits raw
  # observables in exactly the shape the Rust runner emits, so the
  # comparison is a plain equality over two JSON-shaped arrays.
  class RubyExecutor
    # The neutral parity status of each taxonomy class; subclasses
    # precede their base class so the first match wins.
    STATUS_ONLY = [
      [Kobako::TimeoutError, "timeout"],
      [Kobako::MemoryLimitError, "memory_limit"],
      [Kobako::TrapError, "trap"],
      [Kobako::SetupError, "setup"]
    ].freeze

    GUEST_FAILURES = [
      [Kobako::BytecodeError, "bytecode"],
      [Kobako::ServiceError, "service"],
      [Kobako::SandboxError, "sandbox"]
    ].freeze

    # A verb that never ran the guest — a late_bind or a could-not-start
    # failure — carries no Execution, so its observables are all empty.
    NO_OBSERVABLES = {
      "stdout_hex" => "",
      "stderr_hex" => "",
      "stdout_truncated" => false,
      "stderr_truncated" => false,
      "usage" => nil
    }.freeze

    def initialize(wasm_path)
      @wasm_path = wasm_path
    end

    def execute(scenario)
      @builder = SandboxBuilder.new(@wasm_path)
      sandbox = @builder.build(scenario)
      scenario.invocations.map { |invocation| observe(sandbox, invocation) }
    end

    private

    def observe(sandbox, invocation)
      status, execution = invoke(sandbox, invocation)
      status.merge(observables(execution))
    end

    # Run the verb, pairing its status with the run's Execution — the object
    # the observables read from. A guest failure raises a taxonomy error
    # carrying the same Execution on +#execution+; late_bind runs no guest.
    def invoke(sandbox, invocation)
      case invocation.fetch(:verb)
      when "eval" then capture_outcome { eval_verb(sandbox, invocation) }
      when "run" then capture_outcome { run_verb(sandbox, invocation) }
      when "late_bind" then [late_bind(sandbox, invocation), nil]
      else raise ArgumentError, "unknown invocation verb: #{invocation.inspect}"
      end
    end

    # An +eval+ with an optional +overrides+ list runs the per-eval override
    # block, binding each override's stub at its declared path for this
    # invocation only — the +ctx.bind+ parity to the Rust runner's +eval_with+.
    def eval_verb(sandbox, invocation)
      overrides = invocation[:overrides]
      return sandbox.eval(invocation.fetch(:source)) unless overrides

      sandbox.eval(invocation.fetch(:source)) { |ctx| bind_overrides(ctx, overrides) }
    end

    # Tagged +run+ arguments and keyword arguments; an +opaque+ tag in
    # either position becomes a labeled host object the encoding
    # auto-wraps into a capability Handle. An optional +overrides+ list runs
    # the per-eval override block on the +#run+ path — the +ctx.bind+ parity
    # to the Rust runner's +run_with+.
    def run_verb(sandbox, invocation)
      args = (invocation[:args] || []).map { |tagged| ValueTags.untag(tagged) }
      kwargs = (invocation[:kwargs] || {}).transform_values { |tagged| ValueTags.untag(tagged) }
      target = invocation.fetch(:target)
      overrides = invocation[:overrides]
      return sandbox.run(target, *args, **kwargs) unless overrides

      sandbox.run(target, *args, **kwargs) { |ctx| bind_overrides(ctx, overrides) }
    end

    # Bind each override's stub at its declared path on the invocation's
    # Context — the shared body of the +#eval+ / +#run+ override block.
    def bind_overrides(ctx, overrides)
      overrides.each { |override| ctx.bind(override.fetch(:path), @builder.build_stub(override[:methods])) }
    end

    def capture_outcome
      execution = yield
      [{ "status" => "ok", "value" => ValueTags.tag(execution.value) }, execution]
    rescue Kobako::Error => e
      [classify(e), e.execution]
    end

    # A registration refused after the first invocation surfaces the
    # seal (B-33); the Ruby surface spells the refusal ArgumentError.
    def late_bind(sandbox, invocation)
      sandbox.bind(invocation.fetch(:name), Object.new)
      { "status" => "ok", "value" => ValueTags.tag(nil) }
    rescue ArgumentError
      { "status" => "sealed" }
    end

    def classify(error)
      status_only = STATUS_ONLY.find { |(klass, _)| error.is_a?(klass) }
      return { "status" => status_only.last } if status_only

      carrier = GUEST_FAILURES.find { |(klass, _)| error.is_a?(klass) }
      raise error unless carrier

      failure(carrier.last, error)
    end

    def failure(status, error)
      { "status" => status, "class" => error.klass, "message" => error.message }
    end

    # The run's observables, read off its Execution; a verb that ran no
    # guest (execution is +nil+) reports the empty readout.
    def observables(execution)
      return NO_OBSERVABLES if execution.nil?

      {
        "stdout_hex" => execution.stdout.unpack1("H*"),
        "stderr_hex" => execution.stderr.unpack1("H*"),
        "stdout_truncated" => execution.stdout_truncated?,
        "stderr_truncated" => execution.stderr_truncated?,
        "usage" => { "wall_time" => execution.usage.wall_time, "memory_peak" => execution.usage.memory_peak }
      }
    end
  end
end
