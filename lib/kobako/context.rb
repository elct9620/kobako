# frozen_string_literal: true

require_relative "capture"
require_relative "codec"
require_relative "errors"
require_relative "execution"
require_relative "unresolved"
require_relative "outcome"
require_relative "usage"
require_relative "transport"
require_relative "catalog"

module Kobako
  # Kobako::Context — the per-invocation driver behind a single +Sandbox#eval+
  # / +#run+. Each Context owns a fresh +Catalog::Handles+ table and drives one
  # guest invocation: it builds the dispatch handler, runs the guest, records
  # the run's captures and usage, and decodes the outcome. One Context per
  # invocation keeps concurrent evals shared-nothing — no per-invocation state
  # lives on the reusable +Sandbox+.
  class Context
    # The +Kobako::Usage+ value object for this invocation; +Usage::EMPTY+
    # until the guest has run.
    attr_reader :usage

    # Build a Context over the Sandbox-owned config — the +Runtime+, the
    # sealed +Catalog::Services+ / +Catalog::Snippets+ registries, and the
    # +Catalog::Extensions+ whose callable backends this Context resolves for
    # its own run. The +Catalog::Handles+ table is this invocation's own, so
    # guest→host dispatch and host→guest auto-wrap share one allocator scoped
    # to the run; the resolved provider map is likewise per-invocation, so
    # concurrent invocations never share mutable state.
    def initialize(runtime:, services:, snippets:, extensions:)
      @runtime = runtime
      @services = services
      @snippets = snippets
      @extensions = extensions
      @resolved = {} # : Hash[String, untyped]
      @overrides = {} # : Hash[String, untyped]
      @spent = false
      @handler = Catalog::Handles.new
      @stdout_capture = @stderr_capture = Capture::EMPTY
      @usage = Usage::EMPTY
    end

    # Override the object bound at an already-declared +path+ for this
    # invocation only — the per-eval hook the +#eval+ / +#run+ block uses to
    # fill a fillable or shadow any static / per-invocation binding. +path+
    # must name a declared (Frame 1) binding; overriding an undeclared path
    # raises +ArgumentError+ so the Frame 1 key set stays fixed. Valid only
    # while the block runs — the Context is spent once the block returns, so a
    # captured +ctx+ used afterward raises. Returns +self+.
    def bind(path, object)
      raise "Kobako::Context is spent; ctx.bind is only valid inside the #eval / #run block" if @spent

      key = path.to_s
      raise ArgumentError, "cannot override undeclared path #{key.inspect}" unless @services.bound?(key)

      @overrides[key] = object
      self
    end

    # Resolve a Service +path+ to the object backing it this invocation,
    # layering the per-eval +ctx.bind+ overrides over this Context's
    # per-invocation provider results over the Sandbox's static base bindings.
    # An unbound path raises +KeyError+; a fillable left unfilled resolves to
    # +Kobako::Unresolved+ and is reported the same way, so an unresolved
    # capability fails closed as an undefined target rather than dispatching to
    # the sentinel. The Dispatcher maps either +KeyError+ to an
    # undefined-target wire fault. Internal — the per-invocation dispatch
    # handler is the sole caller.
    def lookup(path)
      key = path.to_s
      object = @overrides.fetch(key) { @resolved.fetch(key) { @services.lookup(path) } }
      raise KeyError, "service #{path} is declared but unresolved this invocation" if Unresolved.equal?(object)

      object
    end

    # Bytes the guest wrote to stdout during this invocation as a UTF-8 String,
    # clipped at the cap; the content never carries a truncation sentinel, so
    # use +#stdout_truncated?+ to observe overflow.
    def stdout = @stdout_capture.bytes

    # Bytes the guest wrote to stderr during this invocation. Mirror of #stdout.
    def stderr = @stderr_capture.bytes

    # Returns +true+ iff stdout reached its cap during this invocation.
    def stdout_truncated? = @stdout_capture.truncated?

    # Returns +true+ iff stderr reached its cap during this invocation.
    def stderr_truncated? = @stderr_capture.truncated?

    # Execute a guest mruby source string in a fresh +mrb_state+ and return the
    # decoded last expression. A given +block+ runs first, receiving this
    # Context to collect +ctx.bind+ overrides before the guest drives.
    def eval(code, &block)
      collect_overrides(&block) if block
      invoke!(:eval) do
        @runtime.eval(dispatch_handler, @services.encode, code.b, @snippets.encode)
      end
    end

    # Dispatch a +Transport::Run+ envelope into a preloaded entrypoint and
    # return the decoded result. A given +block+ runs first, receiving this
    # Context to collect +ctx.bind+ overrides before the guest drives.
    def run(run_envelope, &block)
      collect_overrides(&block) if block
      invoke!(:run) do
        @runtime.run(dispatch_handler, @services.encode, @snippets.encode, run_envelope.encode(@handler))
      end
    end

    private

    # Run the per-eval override +block+, handing it this Context so it can call
    # +ctx.bind+, then spend the Context so a captured +ctx+ used after the
    # block raises. A block that raises propagates before the guest drives, so
    # the guest never runs and no Execution is produced.
    def collect_overrides
      yield self
    ensure
      @spent = true
    end

    # Build this invocation's guest→host dispatch handler — a +Proc+ routing
    # each guest→host call through the stateless +Transport::Dispatcher+,
    # capturing this Context as the path resolver (its +#lookup+ layers the
    # per-invocation providers over the static bindings) plus +@handler+. Handed to
    # +Runtime#eval+ / +#run+ as a call argument, so the Runtime holds no
    # dispatch state and the +Proc+ stays GC-rooted as a live argument for the
    # synchronous call. The ext hands the +Proc+ a per-dispatch +guest_yielder+
    # — a +String → String+ callable that re-enters the in-flight guest to run
    # a yielded block — which the +Dispatcher+ forwards to the
    # +Transport::Yielder+ it builds for the call.
    def dispatch_handler
      lambda do |request_bytes, guest_yielder|
        Transport::Dispatcher.dispatch(request_bytes, self, @handler, guest_yielder)
      end
    end

    # Record this invocation's usage and both output captures from the ext
    # +Snapshot+. Every Snapshot carries them — value return or trap alike — so
    # +#usage+ / +#stdout+ / +#stderr+ stay readable after a rescued trap.
    def populate_observability!(snapshot)
      @usage = Usage.new(wall_time: snapshot.wall_time, memory_peak: snapshot.memory_peak)
      @stdout_capture = Capture.new(bytes: snapshot.stdout, truncated: snapshot.stdout_truncated?)
      @stderr_capture = Capture.new(bytes: snapshot.stderr, truncated: snapshot.stderr_truncated?)
    end

    # Pick the +TrapError+ subclass to re-raise based on +err+'s actual class.
    # Cap-trap subclasses (+TimeoutError+ / +MemoryLimitError+) preserve their
    # named identity; everything else collapses to the base +Kobako::TrapError+,
    # so #invoke! can add the verb prefix without erasing the named subclass.
    def trap_class_for(err)
      case err
      when TimeoutError     then TimeoutError
      when MemoryLimitError then MemoryLimitError
      else TrapError
      end
    end

    # Build the +TrapError+-family exception for a trapped +Snapshot+ from its
    # neutral trap kind, tagged with the verb — the cap subclasses
    # (+TimeoutError+ / +MemoryLimitError+) keep their identity, every other
    # engine fault is the base +TrapError+.
    def trap_error_for(snapshot, verb)
      klass = case snapshot.trap_kind
              when :timeout      then TimeoutError
              when :memory_limit then MemoryLimitError
              else TrapError
              end
      klass.new("Sandbox##{verb} failed: #{snapshot.trap_message}")
    end

    # Freeze this run's observables plus +value+ (+nil+ on a failed run) into
    # the read-only +Execution+ the caller receives or the error carries.
    def build_execution(value)
      Execution.new(value: value, usage: @usage, stdout: @stdout_capture, stderr: @stderr_capture)
    end

    # Decode a completed run's outcome into its +Execution+. A Capability
    # Handle in the result is restored to its host object first. Decode sits in
    # the rescue so a wire-violation trap or a Panic envelope both attach this
    # run's Execution, just like a guest-call trap does.
    def settle_outcome(snapshot, verb)
      value, carried = Codec.track_handles { Outcome.decode(snapshot.outcome) }
      value = Codec::HandleWalk.deep_restore(value, @handler) if carried
      build_execution(value)
    rescue Kobako::TrapError => e
      raise trap_class_for(e).new("Sandbox##{verb} failed: #{e.message}").with_execution(build_execution(nil))
    rescue Kobako::SandboxError, Kobako::ServiceError => e
      raise e.with_execution(build_execution(nil))
    end

    # Drive one invocation and settle it into a frozen +Execution+. +verb+
    # tags the TrapError message so the failing export is identifiable. This
    # invocation's callable Extension backends are resolved first — before the
    # guest runs — so a provider that raises propagates unwrapped and leaves
    # the guest unrun. Usage and captures are recorded before the trap check,
    # so a trapped Snapshot's error carries them just like a completed run's
    # return value. A could-not-start fault raises straight from the guest call
    # with no Snapshot, so it gains only the verb prefix and carries no
    # Execution.
    def invoke!(verb)
      @resolved = @extensions.resolve
      begin
        snapshot = yield
      rescue Kobako::TrapError => e
        raise trap_class_for(e), "Sandbox##{verb} failed: #{e.message}"
      end
      populate_observability!(snapshot)
      return settle_outcome(snapshot, verb) unless snapshot.trapped?

      raise trap_error_for(snapshot, verb).with_execution(build_execution(nil))
    end
  end
end
