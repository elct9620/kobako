# frozen_string_literal: true

module Parity
  # Assembles a +Kobako::Sandbox+ from a Scenario's declarative setup —
  # caps, Namespaces, Service stubs, preloads. Invocation-side
  # observation stays in +RubyExecutor+; the Rust runner interprets the
  # same closed tag sets on its side.
  class SandboxBuilder
    # The closed stub-behavior set, one builder per tag; the Rust
    # runner's StubMember is the other interpreter of the same tags.
    # +echo_positional+ takes no keyword arguments on purpose — kwargs
    # on the wire must fail its parameter binding; +yield_each+ yields
    # each positional argument and returns the array of block results;
    # +opaque+ hands back the same labeled non-wire object on every
    # call so identity is observable, and +read_label+ reads it off a
    # (possibly Array-nested) restored argument.
    STUB_BODIES = {
      "echo" => ->(_behavior) { ->(arg = nil, *, **) { arg } },
      "echo_positional" => ->(_behavior) { ->(arg = nil) { arg } },
      "value" => lambda { |behavior|
        constant = ValueTags.untag(behavior.fetch(:value))
        ->(*, **) { constant }
      },
      "raise" => lambda { |behavior|
        message = behavior.fetch(:message, "stub failure")
        ->(*, **) { raise message }
      },
      "yield_each" => ->(_behavior) { ->(*args, **, &blk) { args.map { |arg| blk.call(arg) } } },
      "opaque" => lambda { |behavior|
        constant = OpaqueObject.new(behavior.fetch(:label))
        ->(*, **) { constant }
      },
      "read_label" => lambda { |_behavior|
        lambda { |arg, *, **|
          arg = arg.first while arg.is_a?(Array)
          arg.label
        }
      }
    }.freeze

    def initialize(wasm_path)
      @wasm_path = wasm_path
    end

    def build(scenario)
      sandbox = Kobako::Sandbox.new(wasm_path: @wasm_path, **sandbox_options(scenario.options))
      scenario.defines.each { |name| sandbox.define(name) }
      scenario.services.each { |service| bind_service(sandbox, service) }
      scenario.preloads.each { |preload| apply_preload(sandbox, preload) }
      sandbox
    end

    private

    def bind_service(sandbox, service)
      sandbox.define(service.fetch(:namespace))
             .bind(service.fetch(:member), stub_object(service[:methods]))
    end

    # The closed preload-kind set. Snippet failures are invocation-time
    # observables (replay), so a preload here never raises on a
    # well-formed scenario.
    def apply_preload(sandbox, preload)
      case preload.fetch(:kind)
      when "source" then sandbox.preload(code: preload.fetch(:code), name: preload.fetch(:name))
      when "bytecode" then sandbox.preload(binary: [preload.fetch(:hex)].pack("H*"))
      else raise ArgumentError, "unknown preload kind: #{preload.inspect}"
      end
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

    def stub_body(behavior)
      builder = STUB_BODIES.fetch(behavior.fetch(:behavior)) do
        raise ArgumentError, "unknown stub behavior: #{behavior.inspect}"
      end
      builder.call(behavior)
    end
  end
end
