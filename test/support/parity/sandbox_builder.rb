# frozen_string_literal: true

module Parity
  # Assembles a +Kobako::Sandbox+ from a Scenario's declarative setup —
  # caps, Services, preloads, flags, Extensions, mounts, networks.
  # Invocation-side observation stays in +RubyExecutor+; the Rust runner
  # interprets the same closed tag sets on its side.
  class SandboxBuilder
    # Each scenario part, in the order it registers, and what registers one item.
    PARTS = {
      services: :bind_service, preloads: :apply_preload, flags: :apply_flag,
      extensions: :install_extension, mounts: :apply_mount, networks: :apply_network
    }.freeze

    def initialize(wasm_path)
      @wasm_path = wasm_path
    end

    def build(scenario)
      sandbox = Kobako::Sandbox.new(wasm_path: @wasm_path, **sandbox_options(scenario.options))
      register(sandbox, scenario)
      sandbox
    end

    private

    def register(sandbox, scenario)
      PARTS.each do |part, apply|
        scenario.public_send(part).each { |item| send(apply, sandbox, item) }
      end
    end

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
      path = backend.fetch(:path)
      case backend.fetch(:provider)
      when "fixed" then Kobako::Extension::Backend.new(path: path, object: Stubs.build(methods))
      when "per_invocation" then Kobako::Extension::Backend.new(path: path, provider: -> { Stubs.build(methods) })
      else raise ArgumentError, "unknown provider kind: #{backend.inspect}"
      end
    end

    def bind_service(sandbox, service)
      return sandbox.bind(service.fetch(:name)) if service[:fillable]

      sandbox.bind(service.fetch(:name), Stubs.build(service[:methods], service[:exposed]))
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

    # Whether the guest recognises a flag is an invocation-time observable,
    # so setting one here never raises on a well-formed scenario.
    def apply_flag(sandbox, flag)
      sandbox.flag(flag.fetch(:name), flag.fetch(:value), advisory: flag.fetch(:advisory))
    end

    # A scenario mounts a Tmpfs seeded from its files, the one kind both
    # frontends build from plain data.
    def apply_mount(sandbox, mount)
      filesystem = Kobako::Filesystem::Tmpfs.new(mount.fetch(:files))
      sandbox.mount(mount.fetch(:at), filesystem, access: mount.fetch(:access).to_sym)
    end

    # A scenario's network entry holds endpoints only, the one role both
    # frontends build from plain data.
    def apply_network(sandbox, network)
      sandbox.network(network.fetch(:name).to_sym) do |net|
        network.fetch(:endpoints).each do |endpoint|
          net.endpoint endpoint.fetch(:host), port: endpoint.fetch(:port),
                                              protocol: endpoint.fetch(:protocol).to_sym,
                                              addresses: endpoint.fetch(:addresses).map(&:to_sym)
        end
      end
    end

    # Scenario caps ride in the runner's neutral spelling; translate
    # to the Ruby keyword surface (ms → seconds, profile → Symbol).
    def sandbox_options(options)
      translated = options.slice(:memory_limit, :stdout_limit, :stderr_limit, :open_file_limit)
      translated[:timeout] = options[:timeout_ms] / 1000.0 if options[:timeout_ms]
      translated[:profile] = options[:profile].to_sym if options[:profile]
      translated
    end
  end
end
