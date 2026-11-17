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

    def_delegators :@options, :timeout, :setup_timeout, :memory_limit, :stdout_limit, :stderr_limit,
                   :open_file_limit, :open_connection_limit, :connection_limit, :profile, :gvl

    # Build a Sandbox on the Guest Binary at +wasm_path+, the one bundled
    # with the gem by default. The other keywords are the options
    # SandboxOptions describes.
    #
    # Raises TypeError for an option of another class, RangeError for a cap
    # past what the host counts, ArgumentError for any other invalid option,
    # and SetupError when the runtime cannot be built or provides less
    # isolation than +profile+ asks for.
    def initialize(wasm_path: nil, **)
      @wasm_path = wasm_path || Kobako::Runtime.default_path
      @options = SandboxOptions.new(**Kobako::Runtime.default_options, **)
      @services = Kobako::Catalog::Services.new
      @snippets = Catalog::Snippets.new
      @flags = Catalog::Flags.new
      @extensions = Catalog::Extensions.new
      @grants = Catalog::Grants.new
      @sealed = false
      @runtime = build_runtime
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
    # Raises TypeError for a path that is no Symbol or String, ArgumentError
    # for a malformed path, a path under the reserved
    # +Kobako+ namespace, or a path that collides with an existing binding,
    # and FrozenError for a call after the first invocation.
    def bind(path, object = Unresolved)
      refuse_once_invoked("bind")
      @services.bind(path, object)
      self
    end

    # Install Extensions, each guest source paired with an optional host
    # backend. Any object answering +name+, +source+, +backend+ and
    # +depends_on+ works; Extension is the bundled one. Returns +self+.
    #
    # Raises TypeError for an Extension of another shape, FrozenError for a
    # call after the first invocation, and, at the first invocation,
    # ArgumentError for a missing dependency.
    def install(*extensions)
      refuse_once_invoked("install")

      extensions.each { |extension| @extensions.install(extension, snippets: @snippets, services: @services) }
      self
    end

    # Mount +filesystem+, a Kobako::Filesystem, at the absolute guest path
    # +at+, where the guest's +File+ reaches it. +access+ is +:read+ or
    # +:read_write+; a write beneath a read-only mount fails as
    # <tt>Errno::EROFS</tt> in the guest. Returns +self+.
    #
    # A given block judges each open and each change +access+ allows, asked
    # with the guest path and +:read+ or +:read_write+, and can only refuse
    # more; an open that may create asks +:read_write+ first, then what it
    # opens for. A falsy answer,
    # or a StandardError, fails that call as <tt>Errno::EACCES</tt> in the
    # guest and leaves the script running.
    #
    # A path may sit beneath another mount, and the guest reaches the
    # innermost one.
    #
    # Raises TypeError for a path that is no String or something other than
    # a Filesystem, ArgumentError for a malformed or duplicate path or an
    # unknown access, and FrozenError for a call after the first invocation.
    def mount(at, filesystem, access: :read, &policy)
      refuse_once_invoked("mount")
      @grants.register(Catalog::Mount.new(at: at, access: access, filesystem: filesystem, policy: policy))
      self
    end

    # Give the guest the network entry +entry+ under +name+, any object
    # answering any of +endpoints+, +credentials+, +resolve?+, +connect?+, and
    # +request?+. A given block yields a Kobako::Network::Builder adding to
    # +entry+, so the block's +self+ stays the caller's. Each entry's endpoints
    # and credentials are read afresh for every invocation. Its +resolve?+ is
    # handed each name the guest looks up, its +connect?+ each
    # Kobako::Network::Connection, and its +request?+ each
    # Kobako::Network::Request; any entry refusing refuses. Returns +self+.
    #
    # Raises TypeError for a name that is no Symbol or String, ArgumentError
    # for an empty or duplicate name, an endpoint with an
    # empty host, a port outside 1 to 65535, or an unknown protocol, and
    # FrozenError for a call after the first invocation. An
    # endpoint the ruleset refuses, such as a wildcard leading to +:private+,
    # raises ArgumentError from the invocation that compiles it.
    def network(name, entry = nil, &builder)
      refuse_once_invoked("network")
      @grants.register(Catalog::Network.new(name: name, entry: entry, &builder))
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
    # Raises TypeError for a snippet part of another class, ArgumentError for
    # a malformed or duplicate snippet, and FrozenError for a call after the
    # first invocation. A snippet that fails to load fails every
    # invocation with SandboxError, or BytecodeError for bytecode.
    def preload(code: nil, name: nil, binary: nil)
      refuse_once_invoked("preload")

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
    # Raises TypeError for a name or value of another class, ArgumentError for
    # a duplicate flag, and FrozenError for a call after the first invocation.
    def flag(name, value = "", advisory: false)
      refuse_once_invoked("flag")

      @flags.register(name, value, advisory: advisory)
      self
    end

    # Call +call+ on the preloaded top-level constant +target+ with +args+
    # and +kwargs+, and return the Execution. A given block receives the
    # invocation's Context before the guest runs.
    #
    # Raises TypeError when +target+ is not a Symbol or String, a keyword is
    # not a Symbol, or an argument holds a Hash key or a reflective object
    # that cannot cross, and ArgumentError when +target+ is not a constant
    # name, the arguments carry a Handle, or an argument nests past what the
    # wire carries. A failed run raises as #eval does.
    def run(target, *args, **kwargs, &block)
      request = Transport::Run.new(entrypoint: target, args: args, kwargs: kwargs)
      Codec.within_value_budget(@options.value_budget) { begin_invocation.run(request, &block) }
    end

    # Evaluate +code+, mruby source as a String, and return the Execution,
    # whose +value+ is the last expression. A given block receives the
    # invocation's Context before the guest runs.
    #
    # Raises TypeError when +code+ is no String, and ArgumentError when it is
    # not UTF-8 text, before the guest runs; TrapError when the engine
    # stopped the guest, a deadline or memory budget included; SandboxError
    # when the guest failed, the Guest Binary not starting the run included;
    # and ServiceError when a Service call failed and the guest left it
    # unrescued.
    def eval(code, &block)
      raise TypeError, "code must be a String, got #{code.class}" unless code.is_a?(String)

      source = code.dup.force_encoding(Encoding::UTF_8)
      raise ArgumentError, "code must be UTF-8 text" unless source.valid_encoding?

      Codec.within_value_budget(@options.value_budget) { begin_invocation.eval(source, &block) }
    end

    private

    def build_runtime
      Kobako::Runtime.from_path(@wasm_path, **@options.to_h)
    end

    # The one place the Preamble's parts are put together, for a compile
    # and for every invocation alike.
    def preamble(snippets)
      [@services.paths, snippets, @flags.entries]
    end

    # Registration closes at the first invocation, so every invocation ships
    # the same Services, snippets, flags, Extensions and mounts.
    def refuse_once_invoked(verb)
      raise FrozenError, "cannot #{verb} #{@sealed}" if @sealed
    end

    # Close registration, saying why for the refusal that follows.
    def seal(reason)
      @sealed = reason
    end

    # Sealing before the dependency check keeps a Sandbox whose check failed
    # closed to further registration. The sealed snippets are compiled once,
    # before the first invocation replays them. Per-invocation state lives on
    # the Context it answers, so nothing an invocation does is written back
    # onto the shared config.
    def begin_invocation
      warn_release_on_main_thread
      seal("after first Sandbox invocation")
      @extensions.assert_dependencies
      @snippets.compile { |entries| @runtime.compile(preamble(entries)) }
      Context.new(runtime: @runtime, services: @services, extensions: @extensions, grants: @grants,
                  preamble: preamble(@snippets.entries))
    end

    # Ruby runs a signal's trap handler on the main Thread alone, and one that
    # raises as a run releasing the GVL takes it back after a Service call
    # leaves the run where it cannot unwind. The first such run warns once.
    def warn_release_on_main_thread
      MainThreadRelease.warn_once if @options.gvl == :release && Thread.current == Thread.main
    end

    # The process's one warning that a run releasing the GVL ran on the main
    # Thread.
    module MainThreadRelease
      MESSAGE = "Kobako::Sandbox with gvl: :release runs on the main Thread, where a trap handler " \
                "that raises or exits can stop the run where it cannot unwind; run it on another " \
                "Thread, or keep trap handlers to setting a flag"
      @warned = false

      def self.warn_once
        return if @warned

        @warned = true
        warn(MESSAGE)
      end
    end
    private_constant :MainThreadRelease
  end
end
