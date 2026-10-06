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
  # One invocation, as the block given to Sandbox#eval or Sandbox#run sees it.
  # The block runs before the guest does, so it can choose the objects this
  # invocation alone reaches:
  #
  #   sandbox.eval("Req::Current.user_id") { |ctx| ctx.bind("Req::Current", request) }
  class Context
    def initialize(runtime:, services:, snippets:, extensions:) # :nodoc:
      @runtime = runtime
      @services = services
      @snippets = snippets
      @extensions = extensions
      @resolved = {} # : Hash[String, Transport::Exposure]
      @overrides = {} # : Hash[String, Transport::Exposure]
      @spent = false
      @handler = Catalog::Handles.new
      @stdout_capture = @stderr_capture = Capture::EMPTY
      @usage = Usage::EMPTY
    end

    # Use +object+ for the Service at +path+ during this invocation only,
    # filling a fillable Service or shadowing a bound one. Returns +self+.
    #
    # Raises ArgumentError when +path+ was never bound on the Sandbox, or when
    # called after the block has returned.
    def bind(path, object)
      raise ArgumentError, "Kobako::Context is spent; ctx.bind is only valid inside the #eval / #run block" if @spent

      key = path.to_s
      raise ArgumentError, "cannot override undeclared path #{key.inspect}" unless @services.bound?(key)

      @overrides[key] = Transport::Exposure.of(object)
      self
    end

    # A fillable left unfilled raises like an unbound path, so the guest's
    # call fails closed instead of reaching the sentinel.
    def lookup(path) # :nodoc:
      key = path.to_s
      exposure = @overrides.fetch(key) { @resolved.fetch(key) { @services.lookup(path) } }
      raise KeyError, "service #{path} is declared but unresolved this invocation" if Unresolved.equal?(exposure.object)

      exposure
    end

    def eval(code, &block) # :nodoc:
      collect_overrides(&block) if block
      invoke!(:eval) do
        @runtime.eval(dispatch_handler, @services.paths, code.b, @snippets.entries)
      end
    end

    def run(request, &block) # :nodoc:
      collect_overrides(&block) if block
      invoke!(:run, entrypoint: request.entrypoint) do
        @runtime.run(dispatch_handler, @services.paths, @snippets.entries,
                     request.entrypoint.to_s, request.payload(@handler))
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
      lambda do |target, method_name, block_given, payload, guest_yielder|
        call = Transport::Call.new(target: target, method_name: method_name,
                                   block_given: block_given, payload: payload)
        Transport::Dispatcher.dispatch(call, self, @handler, guest_yielder)
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
    # +failed+ records the two apart so a +nil+ +value+ from a successful run
    # stays distinct from a failed one.
    def build_execution(value, failed:)
      Execution.new(value: value, usage: @usage, stdout: @stdout_capture, stderr: @stderr_capture, failed: failed)
    end

    # Settle a completed run's outcome into its +Execution+. A Capability
    # Handle in the result is restored to its host object first. The settle
    # sits in the rescue so a wire-violation trap or a Panic both attach this
    # run's Execution, just like a guest-call trap does. +entrypoint+ is the
    # name +#run+ asked for, which the host knows and the wire never carries,
    # so an unresolved one names itself on the error it raises.
    def settle_outcome(snapshot, verb, entrypoint)
      kind, payload, panic = snapshot.outcome
      value, carried = Codec.track_handles { Outcome.reify(kind, payload, panic, entrypoint: entrypoint) }
      value = Codec::HandleWalk.deep_restore(value, @handler) if carried
      build_execution(value, failed: false)
    rescue Kobako::TrapError => e
      raise trap_class_for(e).new("Sandbox##{verb} failed: #{e.message}").with_execution(build_execution(nil,
                                                                                                         failed: true))
    rescue Kobako::SandboxError, Kobako::ServiceError => e
      raise e.with_execution(build_execution(nil, failed: true))
    end

    # Drive one invocation and settle it into a frozen +Execution+. +verb+
    # tags the TrapError message so the failing export is identifiable. This
    # invocation's callable Extension backends are resolved first — before the
    # guest runs — so a provider that raises propagates unwrapped and leaves
    # the guest unrun. Usage and captures are recorded before the trap check,
    # so a trapped Snapshot's error carries them just like a completed run's
    # return value. A could-not-start fault ran no invocation at all, so it
    # carries no Execution and gains only the verb prefix.
    def invoke!(verb, entrypoint: nil)
      @resolved = @extensions.resolve.transform_values { |object| Transport::Exposure.of(object) }
      begin
        snapshot = yield
      rescue Kobako::TrapError => e
        raise trap_class_for(e), "Sandbox##{verb} failed: #{e.message}"
      end
      populate_observability!(snapshot)
      return settle_outcome(snapshot, verb, entrypoint) unless snapshot.trapped?

      raise trap_error_for(snapshot, verb).with_execution(build_execution(nil, failed: true))
    end
  end
end
