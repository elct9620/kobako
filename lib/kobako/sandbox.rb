# frozen_string_literal: true

require "forwardable"

require_relative "errors"
require_relative "unresolved"
require_relative "sandbox_options"
require_relative "transport"
require_relative "catalog"
require_relative "context"

module Kobako
  # Runs untrusted mruby scripts in an isolated Wasm instance. Configure it
  # once, binding Services and preloading snippets, then call #eval or #run as
  # often as needed: each call returns an Execution and leaves nothing behind
  # for the next.
  class Sandbox
    extend Forwardable

    # The Guest Binary this Sandbox runs.
    attr_reader :wasm_path

    # The SandboxOptions this Sandbox was built with. Each option is also
    # readable on the Sandbox itself, as in #timeout.
    attr_reader :options

    def_delegators :@options, :timeout, :memory_limit, :stdout_limit, :stderr_limit, :profile, :gvl

    # Build a Sandbox on the Guest Binary at +wasm_path+, the one bundled
    # with the gem by default. The other keywords are the options
    # SandboxOptions describes.
    #
    # Raises ArgumentError for an invalid option, and SetupError when the
    # runtime cannot be built or provides less isolation than +profile+ asks
    # for.
    def initialize(wasm_path: nil, **)
      @wasm_path = wasm_path || Kobako::Runtime.default_path
      @options = SandboxOptions.new(**)
      @services = Kobako::Catalog::Services.new
      @snippets = Catalog::Snippets.new
      @extensions = Catalog::Extensions.new
      @runtime = build_runtime!
    end

    # Make +object+ reachable from the guest as the constant at +path+, a
    # Symbol or String such as <tt>"MyService::KV"</tt>. Returns +self+.
    #
    # The guest reaches the public methods that +object+'s own class and
    # +object+ itself define in source, nothing inherited or mixed in. An
    # +object+ defining a private <tt>respond_to_guest?(name)</tt> decides
    # instead, on every call.
    #
    # Called with only +path+, it declares a fillable Service: the guest sees
    # the constant, and Context#bind supplies the object for each invocation.
    # A call to one left unfilled fails as a ServiceError.
    #
    # Raises ArgumentError for a malformed path, a path that collides with an
    # existing binding, or a call after the first invocation.
    def bind(path, object = Unresolved)
      @services.bind(path, object)
      self
    end

    # Install Extensions, each guest source paired with an optional host
    # backend. Any object answering +name+, +source+, +backend+ and
    # +depends_on+ works; Extension is the bundled one. Returns +self+.
    #
    # Raises ArgumentError for a malformed Extension, a call after the first
    # invocation, or, at the first invocation, a missing dependency.
    def install(*extensions)
      raise ArgumentError, "cannot install after first Sandbox invocation" if @services.sealed?

      extensions.each { |extension| @extensions.install(extension, snippets: @snippets, services: @services) }
      self
    end

    # Register a snippet that every invocation runs, in the order registered,
    # before its own code. Returns +self+.
    #
    # [<tt>preload(code: source, name: Name)</tt>]
    #   mruby source, named <tt>(snippet:Name)</tt> in backtraces.
    # [<tt>preload(binary: bytes)</tt>]
    #   Precompiled RITE bytecode.
    #
    # Raises ArgumentError for a malformed or duplicate snippet, or a call
    # after the first invocation. A snippet that fails to load fails every
    # invocation with SandboxError, or BytecodeError for bytecode.
    def preload(code: nil, name: nil, binary: nil)
      raise ArgumentError, "cannot preload after first Sandbox invocation" if @services.sealed?

      @snippets.register(code: code, name: name, binary: binary)
      self
    end

    # Call +call+ on the preloaded top-level constant +target+ with +args+
    # and +kwargs+, and return the Execution. A given block receives the
    # invocation's Context before the guest runs.
    #
    # Raises TypeError when +target+ is not a Symbol or String, and
    # ArgumentError when it is not a constant name or the arguments carry a
    # Handle or a non-Symbol keyword. A failed run raises as #eval does.
    def run(target, *args, **kwargs, &block)
      request = Transport::Run.new(entrypoint: target, args: args, kwargs: kwargs)
      new_invocation.run(request, &block)
    end

    # Evaluate +code+, mruby source as a String, and return the Execution,
    # whose +value+ is the last expression. A given block receives the
    # invocation's Context before the guest runs.
    #
    # Raises TrapError when the engine stopped the guest, a deadline or
    # memory budget included; SandboxError when the guest's code failed,
    # +code+ not being a String included; and ServiceError when a Service
    # call failed and the guest left it unrescued.
    def eval(code, &block)
      raise SandboxError, "code must be a String, got #{code.class}" unless code.is_a?(String)

      new_invocation.eval(code, &block)
    end

    private

    # Construct the +Runtime+ with the requested isolation profile and
    # refuse one whose declared posture falls below the request —
    # +SandboxOptions#enforce_floor!+ owns the ladder comparison, so a
    # runtime that cannot honor the request never runs guest code.
    def build_runtime!
      runtime = Kobako::Runtime.from_path(@wasm_path, @options.timeout, @options.memory_limit,
                                          @options.stdout_limit, @options.stderr_limit, @options.profile,
                                          @options.gvl)
      @options.enforce_floor!(runtime.profile)
      runtime
    end

    # Seal the config on the first invocation and return a fresh
    # per-invocation +Context+ for the verb to drive. The Context owns this
    # run's Handle table, resolved Extension backends, captures, and usage, so
    # no per-invocation state is written back onto the shared config.
    def new_invocation
      begin_invocation!
      Context.new(runtime: @runtime, services: @services, snippets: @snippets, extensions: @extensions)
    end

    # Per-invocation prologue on the config tier: seals the Service and
    # Extension registries on the first call (idempotent — asserting Extension
    # dependencies then). The Service seal is the one +#bind+ / +#preload+ /
    # +#install+ all gate on. Per-invocation provider resolution and observable
    # state live on the +Context+, not here.
    def begin_invocation!
      @services.seal!
      @extensions.seal!
    end
  end
end
