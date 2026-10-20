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

    def_delegators :@options, :timeout, :memory_limit, :stdout_limit, :stderr_limit, :open_file_limit, :profile, :gvl

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
      @flags = Catalog::Flags.new
      @extensions = Catalog::Extensions.new
      @grants = Catalog::Grants.new
      @sealed = false
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
    # Raises ArgumentError for a malformed path, a path under the reserved
    # +Kobako+ namespace, a path that collides with an existing binding, or a
    # call after the first invocation.
    def bind(path, object = Unresolved)
      refuse_once_invoked!("bind")
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
      refuse_once_invoked!("install")

      extensions.each { |extension| @extensions.install(extension, snippets: @snippets, services: @services) }
      self
    end

    # Mount +filesystem+, a Kobako::Filesystem, at the absolute guest path
    # +at+, where the guest's +File+ reaches it. +access+ is +:read+ or
    # +:read_write+; a write beneath a read-only mount fails as
    # <tt>Errno::EROFS</tt> in the guest. Returns +self+.
    #
    # A given block decides each open and each change in +access+'s place,
    # called with the guest path and +:read+ or +:read_write+. A falsy answer,
    # or a StandardError, fails that call as <tt>Errno::EACCES</tt> in the
    # guest and leaves the script running.
    #
    # Raises TypeError for a path that is no String or something other than
    # a Filesystem, and ArgumentError for a malformed, duplicate, or nested
    # path, an unknown access, or a call after the first invocation.
    def mount(at, filesystem, access: :read, &policy)
      refuse_once_invoked!("mount")
      @grants.register(Catalog::Mount.new(at: at, access: access, filesystem: filesystem, policy: policy))
      self
    end

    # Register a snippet that every invocation runs, in the order registered,
    # before its own code. Returns +self+.
    #
    # [<tt>preload(code: source, name: Name)</tt>]
    #   mruby source, named <tt>(snippet:Name)</tt> in backtraces.
    # [<tt>preload(binary: bytes)</tt>]
    #   Precompiled RITE bytecode, trusted as host code: the unmodified
    #   compiler output of the mruby release the Guest Binary is built on.
    #
    # Raises ArgumentError for a malformed or duplicate snippet, or a call
    # after the first invocation. A snippet that fails to load fails every
    # invocation with SandboxError, or BytecodeError for bytecode.
    def preload(code: nil, name: nil, binary: nil)
      refuse_once_invoked!("preload")

      @snippets.register(code: code, name: name, binary: binary)
      self
    end

    # Set the flag +name+, a Symbol or String, to +value+ for every
    # invocation; an empty +value+ is a plain switch. A Guest Binary that
    # does not recognise the flag fails each invocation with
    # UnsupportedFlagError, unless +advisory+ says the guest may skip it.
    # Mark a flag advisory only when a script cannot tell it was skipped.
    # Returns +self+.
    #
    # Raises ArgumentError for a malformed or duplicate flag, or a call after
    # the first invocation.
    def flag(name, value = "", advisory: false)
      refuse_once_invoked!("flag")

      @flags.register(name, value, advisory: advisory)
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
    # memory budget included; SandboxError when the guest failed, +code+ not
    # being a String or the Guest Binary not starting the run included; and
    # ServiceError when a Service call failed and the guest left it
    # unrescued.
    def eval(code, &block)
      raise SandboxError, "code must be a String, got #{code.class}" unless code.is_a?(String)

      new_invocation.eval(code, &block)
    end

    private

    # A runtime whose declared posture falls below the request never runs
    # guest code.
    def build_runtime!
      runtime = Kobako::Runtime.from_path(@wasm_path, **@options.to_h)
      @options.enforce_floor!(runtime.profile)
      runtime
    end

    # Per-invocation state lives on the Context, so nothing an invocation
    # does is written back onto the shared config.
    def new_invocation
      begin_invocation!
      Context.new(runtime: @runtime, services: @services, extensions: @extensions, grants: @grants,
                  preamble: preamble(@snippets.entries))
    end

    # The one place the Preamble's parts are put together, for a compile
    # and for every invocation alike.
    def preamble(snippets)
      [@services.paths, snippets, @flags.entries]
    end

    # Registration closes at the first invocation, so every invocation ships
    # the same Services, snippets, flags, Extensions and mounts.
    def refuse_once_invoked!(verb)
      raise ArgumentError, "cannot #{verb} after first Sandbox invocation" if @sealed
    end

    # Sealing before the dependency check keeps a Sandbox whose check failed
    # closed to further registration. The sealed snippets are compiled once,
    # before the first invocation replays them.
    def begin_invocation!
      @sealed = true
      @extensions.assert_dependencies!
      @snippets.compile { |entries| @runtime.compile(*preamble(entries)) }
    end
  end
end
