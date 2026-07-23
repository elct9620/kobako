# frozen_string_literal: true

require "forwardable"

require_relative "capture"
require_relative "codec"
require_relative "errors"
require_relative "outcome"
require_relative "sandbox_options"
require_relative "usage"
require_relative "transport"
require_relative "catalog"

module Kobako
  # Kobako::Sandbox — the user-facing entry point for executing guest mruby
  # scripts inside a wasmtime-hosted Wasm module.
  #
  # The Sandbox owns the +Kobako::Runtime+, the per-Sandbox
  # +Kobako::Catalog::Handles+ shared by guest→host dispatch and host→guest
  # auto-wrap so both name a Handle by the same id, the per-instance
  # +Kobako::Catalog::Services+. Each invocation builds a fresh dispatch
  # +Proc+ and hands it to +Runtime#eval+ / +#run+ as a call argument, so the
  # Runtime holds no dispatch state. The underlying wasmtime Engine and
  # compiled Module are cached at process scope by the native ext and never
  # surface to Ruby — constructing many Sandboxes amortises both costs
  # automatically.
  #
  # Output capture policy: the
  # per-channel cap (+stdout_limit+ / +stderr_limit+) is enforced inside the
  # WASI pipe — the host buffer stops growing at the cap, subsequent guest
  # writes on that channel fail or are dropped, and +#run+ still returns
  # normally. +#stdout+ / +#stderr+ return the captured prefix as a UTF-8
  # String; the byte content never carries a truncation sentinel.
  # +#stdout_truncated?+ / +#stderr_truncated?+ are the only way to observe
  # that the cap was hit.
  class Sandbox
    extend Forwardable

    attr_reader :wasm_path, :options

    # Per-option accessors forward to the immutable +SandboxOptions+ Value
    # Object so the Host App still reads them off Sandbox directly.
    def_delegators :@options, :timeout, :memory_limit, :stdout_limit, :stderr_limit, :profile

    # Returns the bytes the guest wrote to stdout during the most recent
    # invocation as a UTF-8 String, clipped at +stdout_limit+. Empty before
    # any invocation; the byte content never contains a truncation sentinel,
    # so use +#stdout_truncated?+ to observe overflow. Populated on every
    # outcome — including a rescued +TrapError+, after which it holds the
    # bytes written before the trap fired — mirroring +#usage+.
    def stdout = @stdout_capture.bytes

    # Returns the bytes the guest wrote to stderr during the most recent
    # invocation as a UTF-8 String, clipped at +stderr_limit+. Empty before
    # any invocation. Mirror of +#stdout+.
    def stderr = @stderr_capture.bytes

    # Returns +true+ iff stdout capture during the most recent invocation
    # exceeded +stdout_limit+. Resets to +false+ at the start of the next
    # invocation.
    def stdout_truncated? = @stdout_capture.truncated?

    # Returns +true+ iff stderr capture during the most recent invocation
    # exceeded +stderr_limit+. Mirror of +#stdout_truncated?+.
    def stderr_truncated? = @stderr_capture.truncated?

    # Returns the +Kobako::Usage+ value object for the most recent
    # invocation. Carries +wall_time+ (Float seconds the guest export call spent
    # inside wasmtime) and +memory_peak+ (Integer bytes, high-water of
    # the per-invocation +memory.grow+ delta past the entry-time
    # baseline). Returns +Kobako::Usage::EMPTY+ before any invocation;
    # populated on every outcome — including +TrapError+ — so the Host
    # App can read it after rescuing a trap to diagnose budget
    # consumption.
    attr_reader :usage

    # Build a fresh Sandbox.
    #
    # +wasm_path+ is the absolute path to the Guest Binary; defaults to the
    # gem-bundled +data/kobako.wasm+. Every other keyword — the four caps
    # (+stdout_limit+, +stderr_limit+, +timeout+, +memory_limit+) and the
    # requested isolation profile (+profile+) — is forwarded verbatim to
    # +Kobako::SandboxOptions+, which owns the DEFAULT fallbacks and
    # normalisation. The constructed +SandboxOptions+ is exposed as
    # +#options+ and every option remains readable directly on Sandbox via
    # +Forwardable+ delegation. The runtime builds the requested profile —
    # +:hermetic+ (the default) denies the guest ambient time and entropy,
    # +:permissive+ leaves them live — and construction refuses a runtime
    # whose declared profile falls below the request, raising
    # +Kobako::SetupError+ before any invocation entry point runs.
    def initialize(wasm_path: nil, **)
      @wasm_path = wasm_path || Kobako::Runtime.default_path
      @options = SandboxOptions.new(**)
      @handler = Catalog::Handles.new
      @services = Kobako::Catalog::Services.new
      @snippets = Catalog::Snippets.new
      @extensions = Catalog::Extensions.new
      @runtime = build_runtime!
      reset_invocation_state!
    end

    # Bind +object+ as the Service reachable at +path+ — a Symbol or
    # String of one or more +::+-separated constant-form segments
    # (+"MyService::KV"+ or a top-level +"File"+). Returns +self+ for
    # chaining.
    #
    # Raises +ArgumentError+ when a segment is malformed, when +path+
    # collides with an existing binding (a name is a bound Service or a
    # grouping prefix, never both), or when called after the first
    # invocation has sealed Service registration.
    def bind(path, object)
      @services.bind(path, object)
      self
    end

    # Install one or more Extensions — each a guest idiom (+source+) paired
    # with an optional host +backend+, composed onto the Sandbox through
    # +#preload+ and +#bind+. An Extension is any object exposing
    # +name+ / +source+ / +backend+ / +depends_on+; +Kobako::Extension+ is
    # the bundled value type. Returns +self+.
    #
    # Raises +ArgumentError+ for a malformed Extension, a call after the
    # first invocation seals registration, or — at that first invocation —
    # an unmet +depends_on+.
    def install(*extensions)
      raise ArgumentError, "cannot install after first Sandbox invocation" if @services.sealed?

      extensions.each { |extension| @extensions.install(extension, snippets: @snippets, services: @services) }
      self
    end

    # Register a snippet on this Sandbox in one of two forms:
    #
    #   * +preload(code: source, name: Name)+ — +source+ is mruby source
    #     as a +String+ and +Name+ matches +/\A[A-Z]\w*\z/+. Compile
    #     failures surface as +Kobako::SandboxError+ on the first
    #     invocation's replay. The +name+
    #     becomes the snippet's +(snippet:Name)+ backtrace filename and
    #     is the dedupe key that rejects a duplicate +code:+ snippet.
    #   * +preload(binary: bytes)+ — +bytes+ is precompiled RITE
    #     bytecode as a +String+. The canonical name, when present,
    #     lives in the bytecode's embedded +debug_info+ and is resolved
    #     by the guest at load time; the host treats the bytes as
    #     opaque. Structural failures surface as +Kobako::BytecodeError+
    #     on the first invocation.
    #
    # Subsequent invocations (+#eval+ or +#run+) replay every registered
    # snippet — in insertion order — against the fresh +mrb_state+
    # before per-invocation source or entrypoint resolution.
    #
    # Returns +self+ to allow chaining.
    #
    # Raises +ArgumentError+ when neither form's keyword set is
    # supplied, when both forms are mixed (e.g., +code:+ and +binary:+
    # together, or +binary:+ paired with +name:+), when +code+ / +bytes+
    # is not a +String+, when +name+ does not match the constant
    # pattern, when +name+ duplicates an already-registered +code:+ form
    # snippet, or when called after the first invocation has sealed the
    # snippet table.
    def preload(code: nil, name: nil, binary: nil)
      raise ArgumentError, "cannot preload after first Sandbox invocation" if @services.sealed?

      @snippets.register(code: code, name: name, binary: binary)
      self
    end

    # Dispatch into a preloaded entrypoint constant. Delegates host
    # pre-flight and wire encoding to +Kobako::Transport::Run+ /
    # +Kobako::Transport::Run#encode+: a non-Symbol/String +target+ raises
    # +TypeError+, while a +target+ failing the constant pattern, a forged
    # +Kobako::Handle+ in +args+ / +kwargs+, or a non-Symbol +kwargs+ key
    # raise +ArgumentError+. The guest resolves +target+ as a top-level
    # constant, calls +#call+ on it with +args+ / +kwargs+, and returns the
    # deserialized result. The first invocation seals the Service registry
    # and snippet table. Runtime errors follow the same three-class
    # taxonomy as +#eval+.
    def run(target, *args, **kwargs)
      run_envelope = Transport::Run.new(entrypoint: target, args: args, kwargs: kwargs)
      invoke!(:run) do
        @runtime.run(dispatch_handler, @services.encode, @snippets.encode, run_envelope.encode(@handler))
      end
    end

    # Execute a guest mruby source string in a fresh +mrb_state+. +code+ is
    # the mruby source as a UTF-8 String. Returns the deserialized last
    # expression of the source.
    #
    # Source delivery uses the WASI stdin three-frame protocol
    # ({docs/wire-codec.md Invocation channels}[link:../../docs/wire-codec.md]):
    # Frame 1 carries the msgpack-encoded preamble (Service registry
    # snapshot), Frame 2 carries the user source UTF-8 bytes, and
    # Frame 3 carries the snippet table registered via +#preload+.
    # Each frame is prefixed by a 4-byte big-endian u32 length; Frame 3 is
    # mandatory-presence — an empty snippet table sends an empty msgpack
    # array, never an absent frame.
    #
    # The first invocation seals the Service registry and snippet table;
    # subsequent +#bind+ / +#preload+ calls raise +ArgumentError+.
    #
    # Raises +Kobako::TrapError+ on a Wasm trap or wire-violation fallback;
    # +Kobako::SandboxError+ when the guest ran to completion but failed
    # (including when +code+ is +nil+ or not a String, or when a preloaded
    # snippet's replay raises); +Kobako::ServiceError+ on an unrescued
    # Service capability failure.
    def eval(code)
      raise SandboxError, "code must be a String, got #{code.class}" unless code.is_a?(String)

      invoke!(:eval) do
        @runtime.eval(dispatch_handler, @services.encode, code.b, @snippets.encode)
      end
    end

    # Reset all per-invocation observable state to its pre-invocation
    # sentinels — both per-channel captures and the per-last-invocation
    # usage record. Shared by +#initialize+ (first-time setup) and
    # +#begin_invocation!+ (between-invocation reset) so both paths agree on
    # what "pre-invocation state" means; +Kobako::Pool+ calls it at checkout
    # so a pooled Sandbox hands over empty output buffers.
    def reset_invocation_state!
      @stdout_capture = Capture::EMPTY
      @stderr_capture = Capture::EMPTY
      @usage = Usage::EMPTY
    end

    private

    # Construct the +Runtime+ with the requested isolation profile and
    # refuse one whose declared posture falls below the request —
    # +SandboxOptions#enforce_floor!+ owns the ladder comparison, so a
    # runtime that cannot honor the request never runs guest code.
    def build_runtime!
      runtime = Kobako::Runtime.from_path(@wasm_path, @options.timeout, @options.memory_limit,
                                          @options.stdout_limit, @options.stderr_limit, @options.profile)
      @options.enforce_floor!(runtime.profile)
      runtime
    end

    # Build this invocation's guest→host dispatch handler — a +Proc+ routing
    # each guest→host call through the stateless +Transport::Dispatcher+,
    # capturing +@services+ / +@handler+. Handed to +Runtime#eval+ / +#run+ as
    # a call argument, so the Runtime holds no dispatch state and the +Proc+
    # stays GC-rooted as a live argument for the synchronous call. The ext
    # hands the +Proc+ a per-dispatch +guest_yielder+ — a +String → String+
    # callable that re-enters the in-flight guest to run a yielded block —
    # which the +Dispatcher+ forwards to the +Transport::Yielder+ it builds
    # for the call.
    def dispatch_handler
      lambda do |request_bytes, guest_yielder|
        Transport::Dispatcher.dispatch(request_bytes, @services, @handler, guest_yielder)
      end
    end

    # Per-invocation prologue. Seals the Service / snippet / Extension
    # registries on first call (idempotent — asserting Extension
    # dependencies then), refreshes each callable Extension backend to this
    # invocation's fresh object, and zeros the per-invocation capability
    # state — capture buffers, truncation predicates, and the
    # +Catalog::Handles+ counter — before the guest runs. The
    # +Catalog::Handles+ itself is held as +@handler+ and never exposed
    # beyond this class — it is not part of the Host App's surface.
    def begin_invocation!
      @services.seal!
      @snippets.seal!
      @extensions.seal!
      @extensions.refresh_backends!(@services)
      @handler.reset!
      reset_invocation_state!
    end

    # Record this invocation's usage and both output captures from the ext
    # +Snapshot+. Every Snapshot carries them — value return or trap alike —
    # so +#usage+ / +#stdout+ / +#stderr+ stay readable after a rescued trap.
    def populate_observability!(snapshot)
      @usage = Usage.new(wall_time: snapshot.wall_time, memory_peak: snapshot.memory_peak)
      @stdout_capture = Capture.new(bytes: snapshot.stdout, truncated: snapshot.stdout_truncated?)
      @stderr_capture = Capture.new(bytes: snapshot.stderr, truncated: snapshot.stderr_truncated?)
    end

    # Pick the +TrapError+ subclass to re-raise based on +err+'s actual
    # class. Cap-trap subclasses (+TimeoutError+ / +MemoryLimitError+)
    # preserve their named identity; everything else collapses to the
    # base +Kobako::TrapError+. The ext already raises the right subclass
    # directly, so this is a pure re-attribution that lets +#invoke!+
    # add the verb prefix without erasing +TimeoutError+ /
    # +MemoryLimitError+.
    def trap_class_for(err)
      case err
      when TimeoutError     then TimeoutError
      when MemoryLimitError then MemoryLimitError
      else TrapError
      end
    end

    # Build the bare +TrapError+-family exception for a trapped +Snapshot+
    # from its neutral trap kind — the cap subclasses (+TimeoutError+ /
    # +MemoryLimitError+) keep their identity, every other engine fault is
    # the base +TrapError+. #invoke! adds the verb prefix uniformly.
    def trap_for(snapshot)
      klass = case snapshot.trap_kind
              when :timeout      then TimeoutError
              when :memory_limit then MemoryLimitError
              else TrapError
              end
      klass.new(snapshot.trap_message)
    end

    # Shared prologue + outcome settling for both invocation verbs. +verb+ is
    # +:eval+ or +:run+; it tags the TrapError message so the failing export
    # is identifiable. The yielded block returns the ext +Snapshot+; usage and
    # captures are recorded from it before the trap check, so +#stdout+ /
    # +#stderr+ / +#usage+ stay readable after a rescued trap. A Capability
    # Handle in a completed outcome is restored to its host object before the
    # value is returned. The single rescue is the trap-translation boundary:
    # a guest trap, a could-not-start fault, and a wire violation all surface
    # as the named TrapError subclass with the verb prefix; a Panic envelope's
    # SandboxError / ServiceError propagates untouched.
    def invoke!(verb)
      begin_invocation!
      snapshot = yield
      populate_observability!(snapshot)
      raise trap_for(snapshot) if snapshot.trapped?

      value, carried_handle = Codec.track_handles { Outcome.decode(snapshot.outcome) }
      carried_handle ? Codec::HandleWalk.deep_restore(value, @handler) : value
    rescue Kobako::TrapError => e
      raise trap_class_for(e), "Sandbox##{verb} failed: #{e.message}"
    end
  end
end
