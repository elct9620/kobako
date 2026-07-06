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
      sandbox = build_sandbox(scenario)
      scenario.invocations.map { |invocation| observe(sandbox, invocation) }
    end

    private

    def build_sandbox(scenario)
      sandbox = Kobako::Sandbox.new(wasm_path: @wasm_path, **sandbox_options(scenario.options))
      scenario.defines.each { |name| sandbox.define(name) }
      scenario.services.each do |service|
        namespace = sandbox.define(service.fetch(:namespace))
        namespace.bind(service.fetch(:member), stub_object(service[:methods]))
      end
      sandbox
    end

    # Scenario caps ride in the runner's neutral spelling; translate
    # to the Ruby keyword surface (ms → seconds, profile → Symbol).
    def sandbox_options(options)
      translated = {}
      translated[:timeout] = options[:timeout_ms] / 1000.0 if options[:timeout_ms]
      %i[memory_limit stdout_limit stderr_limit].each do |cap|
        translated[cap] = options[cap] if options.key?(cap)
      end
      translated[:profile] = options[:profile].to_sym if options[:profile]
      translated
    end

    def stub_object(methods)
      stub = Object.new
      (methods || {}).each do |name, behavior|
        stub.define_singleton_method(name, &stub_body(behavior))
      end
      stub
    end

    # The closed stub-behavior set; the Rust runner's StubMember is
    # the other interpreter of the same tags.
    def stub_body(behavior)
      case behavior.fetch(:behavior)
      when "echo" then ->(arg = nil, *, **) { arg }
      when "value" then constant = ValueTags.untag(behavior.fetch(:value))
                        ->(*, **) { constant }
      when "raise" then message = behavior.fetch(:message, "stub failure")
                        ->(*, **) { raise message }
      else raise ArgumentError, "unknown stub behavior: #{behavior.inspect}"
      end
    end

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
      when "run" then capture_outcome { sandbox.run(invocation.fetch(:target)) }
      when "late_bind" then late_bind(sandbox, invocation)
      else raise ArgumentError, "unknown invocation verb: #{invocation.inspect}"
      end
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
