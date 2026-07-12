# frozen_string_literal: true

module Parity
  # Assembles a +Kobako::Sandbox+ from a Scenario's declarative setup —
  # caps, Services, stubs, preloads. Invocation-side
  # observation stays in +RubyExecutor+; the Rust runner interprets the
  # same closed tag sets on its side.
  class SandboxBuilder
    # The closed stub-behavior set, one builder per tag; the Rust
    # runner's StubReceiver is the other interpreter of the same tags.
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
      },
      "counter" => lambda { |_behavior|
        count = 0
        ->(*, **) { count += 1 }
      }
    }.freeze

    def initialize(wasm_path)
      @wasm_path = wasm_path
    end

    def build(scenario)
      sandbox = Kobako::Sandbox.new(wasm_path: @wasm_path, **sandbox_options(scenario.options))
      scenario.services.each { |service| bind_service(sandbox, service) }
      scenario.preloads.each { |preload| apply_preload(sandbox, preload) }
      scenario.extensions.each { |extension| install_extension(sandbox, extension) }
      sandbox
    end

    private

    # An Extension composes a preloaded source with an optional stub
    # backend. A +fixed+ provider binds one stub for the Sandbox's life; a
    # +per_invocation+ provider hands back a fresh stub each invocation, so
    # a stateful backend (a +counter+) resets between invocations.
    def install_extension(sandbox, extension)
      backend = extension[:backend]
      sandbox.install(
        Kobako::Extension.new(
          name: extension.fetch(:name).to_sym,
          source: extension.fetch(:source),
          backend: backend && extension_backend(backend),
          depends_on: (extension[:depends_on] || []).map(&:to_sym)
        )
      )
    end

    def extension_backend(backend)
      methods = backend[:methods]
      provider =
        case backend.fetch(:provider)
        when "fixed" then stub_object(methods, nil)
        when "per_invocation" then -> { stub_object(methods, nil) }
        else raise ArgumentError, "unknown provider kind: #{backend.inspect}"
        end
      Kobako::Extension::Backend.new(path: backend.fetch(:path), provider: provider)
    end

    def bind_service(sandbox, service)
      sandbox.bind(service.fetch(:name), stub_object(service[:methods], service[:exposed]))
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

    def stub_object(methods, exposed)
      stub = Object.new
      (methods || {}).each do |name, behavior|
        stub.define_singleton_method(name, &stub_body(behavior))
      end
      narrow_guest_surface(stub, exposed) if exposed
      stub
    end

    # A service's optional +exposed+ list is the scenario's
    # +respond_to_guest?+ narrowing: the predicate is defined private,
    # so the guest can never dispatch to it.
    def narrow_guest_surface(stub, exposed)
      allowed = exposed.map(&:to_s)
      stub.define_singleton_method(:respond_to_guest?) { |name| allowed.include?(name.to_s) }
      stub.singleton_class.class_eval { private :respond_to_guest? }
    end

    def stub_body(behavior)
      builder = STUB_BODIES.fetch(behavior.fetch(:behavior)) do
        raise ArgumentError, "unknown stub behavior: #{behavior.inspect}"
      end
      builder.call(behavior)
    end
  end
end
