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
      @handles = Catalog::Handles.new
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
                     request.entrypoint.to_s, request.payload(@handles))
      end
    end

    private

    # The Context is spent once the block returns, so a captured +ctx+ used
    # later raises. A block that raises propagates before the guest drives,
    # so the guest never runs and no Execution is produced.
    def collect_overrides
      yield self
    ensure
      @spent = true
    end

    # Handed to the Runtime as a call argument, so the Runtime holds no
    # dispatch state and the Proc stays GC-rooted for the synchronous call.
    def dispatch_handler
      lambda do |target, method_name, block_given, payload, guest_yielder|
        call = Transport::Call.new(target: target, method_name: method_name,
                                   block_given: block_given, payload: payload)
        Transport::Dispatcher.dispatch(call, self, @handles, guest_yielder)
      end
    end

    # Every Snapshot carries usage and captures, trap or not, so they stay
    # readable after a rescued trap.
    def populate_observability!(snapshot)
      @usage = Usage.new(wall_time: snapshot.wall_time, memory_peak: snapshot.memory_peak)
      @stdout_capture = Capture.new(bytes: snapshot.stdout, truncated: snapshot.stdout_truncated?)
      @stderr_capture = Capture.new(bytes: snapshot.stderr, truncated: snapshot.stderr_truncated?)
    end

    # Every TrapError leaving an invocation names the verb that ran it; the
    # cap subclasses keep their identity through the rebuild.
    def with_verb(err, verb)
      err.class.new("Sandbox##{verb} failed: #{err.message}")
    end

    # +failed+ keeps a +nil+ value from a successful run apart from a failed
    # run.
    def build_execution(value, failed:)
      Execution.new(value: value, usage: @usage, stdout: @stdout_capture, stderr: @stderr_capture, failed: failed)
    end

    # The settle sits in the rescue so a wire-violation trap or a Panic both
    # attach this run's Execution, just like a guest-call trap does.
    # +entrypoint+ is known to the host and never carried by the wire, so an
    # unresolved one names itself on the error it raises.
    def settle_outcome(snapshot, verb, entrypoint)
      kind, payload, panic = snapshot.outcome
      value, carried = Codec.track_handles { Outcome.reify(kind, payload, panic, entrypoint: entrypoint) }
      value = Codec::HandleWalk.deep_restore(value, @handles) if carried
      build_execution(value, failed: false)
    rescue Kobako::TrapError => e
      raise with_verb(e, verb).with_execution(build_execution(nil, failed: true))
    rescue Kobako::SandboxError, Kobako::ServiceError => e
      raise e.with_execution(build_execution(nil, failed: true))
    end

    # Extension backends are resolved before the guest runs, so a provider
    # that raises propagates unwrapped and leaves the guest unrun. A
    # could-not-start fault ran no invocation at all, so it carries no
    # Execution and gains only the verb prefix.
    def invoke!(verb, entrypoint: nil)
      @resolved = @extensions.resolve.transform_values { |object| Transport::Exposure.of(object) }
      begin
        snapshot = yield
      rescue Kobako::TrapError => e
        raise with_verb(e, verb)
      end
      populate_observability!(snapshot)
      trap = snapshot.trap_error
      return settle_outcome(snapshot, verb, entrypoint) unless trap

      raise with_verb(trap, verb).with_execution(build_execution(nil, failed: true))
    end
  end
end
