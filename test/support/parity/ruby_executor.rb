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

    def initialize(wasm_path)
      @wasm_path = wasm_path
    end

    def execute(scenario)
      sandbox = SandboxBuilder.new(@wasm_path).build(scenario)
      scenario.invocations.map { |invocation| observe(sandbox, invocation) }
    end

    private

    def observe(sandbox, invocation)
      invoke(sandbox, invocation).merge(
        "stdout_hex" => sandbox.stdout.unpack1("H*"),
        "stderr_hex" => sandbox.stderr.unpack1("H*"),
        "stdout_truncated" => sandbox.stdout_truncated?,
        "stderr_truncated" => sandbox.stderr_truncated?,
        "usage" => usage_observable(sandbox)
      )
    end

    def invoke(sandbox, invocation)
      case invocation.fetch(:verb)
      when "eval" then capture_outcome { sandbox.eval(invocation.fetch(:source)) }
      when "run" then capture_outcome { sandbox.run(invocation.fetch(:target), *run_args(invocation)) }
      when "late_bind" then late_bind(sandbox, invocation)
      else raise ArgumentError, "unknown invocation verb: #{invocation.inspect}"
      end
    end

    # Tagged +run+ arguments; an +opaque+ tag becomes a labeled host
    # object the encoding auto-wraps into a capability Handle.
    def run_args(invocation)
      (invocation[:args] || []).map { |tagged| ValueTags.untag(tagged) }
    end

    def capture_outcome
      value = yield
      { "status" => "ok", "value" => ValueTags.tag(value) }
    rescue Kobako::Error => e
      classify(e)
    end

    # A registration refused after the first invocation surfaces the
    # seal (B-33); the Ruby surface spells the refusal ArgumentError.
    def late_bind(sandbox, invocation)
      sandbox.define(invocation.fetch(:namespace))
             .bind(invocation.fetch(:member), Object.new)
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

    def usage_observable(sandbox)
      usage = sandbox.usage
      return nil unless usage

      { "wall_time" => usage.wall_time, "memory_peak" => usage.memory_peak }
    end
  end
end
