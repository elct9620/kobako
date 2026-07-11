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
  # +Kobako::Catalog::Handles+, the per-instance
  # +Kobako::Catalog::Namespaces+ (which receives the +Catalog::Handles+ by
  # injection so guest→host dispatch and host→guest auto-wrap share one
  # allocator), and the dispatch +Proc+ / +yield_to_guest+ lambda installed
  # on the Runtime via +Runtime#on_dispatch=+. The underlying wasmtime Engine
  # and compiled Module are cached at process scope by the native ext and
  # never surface to Ruby — constructing many Sandboxes amortises both costs
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
    def stdout
      @stdout_capture.bytes
    end

    # Returns the bytes the guest wrote to stderr during the most recent
    # invocation as a UTF-8 String, clipped at +stderr_limit+. Empty before
    # any invocation. Mirror of +#stdout+.
    def stderr
      @stderr_capture.bytes
    end

    # Returns +true+ iff stdout capture during the most recent invocation
    # exceeded +stdout_limit+. Resets to +false+ at the start of the next
    # invocation.
    def stdout_truncated?
      @stdout_capture.truncated?
    end

    # Returns +true+ iff stderr capture during the most recent invocation
    # exceeded +stderr_limit+. Mirror of +#stdout_truncated?+.
    def stderr_truncated?
      @stderr_capture.truncated?
    end

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
      @services = Kobako::Catalog::Namespaces.new(handler: @handler)
      @snippets = Catalog::Snippets.new
      @runtime = build_runtime!
      install_dispatch_proc!
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
        @runtime.run(@services.encode, @snippets.encode, run_envelope.encode(@handler))
      end
    end

    # Execute a guest mruby source string in a fresh +mrb_state+. +code+ is
    # the mruby source as a UTF-8 String. Returns the deserialized last
    # expression of the source.
    #
    # Source delivery uses the WASI stdin three-frame protocol
    # ({docs/wire-codec.md Invocation channels}[link:../../docs/wire-codec.md]):
    # Frame 1 carries the msgpack-encoded preamble (Namespace / Member
    # registry snapshot), Frame 2 carries the user source UTF-8 bytes, and
    # Frame 3 carries the snippet table registered via +#preload+.
    # Each frame is prefixed by a 4-byte big-endian u32 length; Frame 3 is
    # mandatory-presence — an empty snippet table sends an empty msgpack
    # array, never an absent frame.
    #
    # The first invocation seals the Service registry and snippet table;
    # subsequent +#define+ / +#preload+ calls raise +ArgumentError+.
    #
    # Raises +Kobako::TrapError+ on a Wasm trap or wire-violation fallback;
    # +Kobako::SandboxError+ when the guest ran to completion but failed
    # (including when +code+ is +nil+ or not a String, or when a preloaded
    # snippet's replay raises); +Kobako::ServiceError+ on an unrescued
    # Service capability failure.
    def eval(code)
      raise SandboxError, "code must be a String, got #{code.class}" unless code.is_a?(String)

      invoke!(:eval) do
        @runtime.eval(@services.encode, code.b, @snippets.encode)
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

    # Configure the +Runtime+'s host↔guest dispatch wiring. Registers a
    # dispatch +Proc+ that routes guest→host calls through the stateless
    # +Transport::Dispatcher+, capturing +@services+ / +@handler+ in the
    # closure. The ext hands the +Proc+ a per-dispatch +guest_yielder+ — a
    # +String → String+ callable that re-enters the in-flight guest to run a
    # yielded block — which the +Dispatcher+ forwards to the +Transport::Yielder+
    # it builds for the call. Registered once at construction time so the
    # wasm ext callback can fire without further setup.
    def install_dispatch_proc!
      @runtime.on_dispatch = lambda do |request_bytes, guest_yielder|
        Transport::Dispatcher.dispatch(request_bytes, @services, @handler, guest_yielder)
      end
    end

    # Per-invocation prologue. Seals the Service / snippet registries on
    # first call (idempotent) and zeros the per-invocation capability
    # state — capture buffers, truncation predicates, and the
    # +Catalog::Handles+ counter — before the guest runs. The
    # +Catalog::Handles+ itself is held as +@handler+ and never exposed
    # beyond this class — it is not part of the Host App's surface.
    def begin_invocation!
      @services.seal!
      @handler.reset!
      reset_invocation_state!
    end

    # Read the per-last-invocation +wall_time+ and +memory_peak+ from
    # the ext and wrap them as a +Kobako::Usage+ value object. Runs in
    # the +invoke!+ +ensure+ block so the usage record is populated on
    # every outcome — value return, +Kobako::TrapError+ (including
    # +TimeoutError+ / +MemoryLimitError+), +Kobako::SandboxError+,
    # and +Kobako::ServiceError+. +Runtime#usage+ is the single source for
    # both paths: the figures are stashed in the ext on every outcome, so
    # the readout here also covers the trap path, where +Runtime#eval+ /
    # +#run+ raise instead of returning outcome bytes.
    #
    # The ext-side contract is positional: +Runtime#usage+ yields
    # +[wall_time, memory_peak]+ in +Kobako::Usage+ field order.
    def read_usage!
      wall_time, memory_peak = @runtime.usage
      @usage = Usage.new(wall_time: wall_time, memory_peak: memory_peak)
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

    # Read the per-last-invocation output captures from the ext and wrap
    # them as +Kobako::Capture+ value objects. Runs in the +invoke!+
    # +ensure+ block next to #read_usage! for the same reason: the ext
    # stashes the captures on every outcome, so the readout also covers
    # the trap path, where +Runtime#eval+ / +#run+ raise instead of
    # returning outcome bytes — +#stdout+ / +#stderr+ keep the guest's
    # partial output readable after a rescue.
    #
    # The ext-side contract is positional: +Runtime#captures+ yields
    # +[stdout_bytes, stdout_truncated, stderr_bytes, stderr_truncated]+.
    def read_captures!
      stdout_bytes, stdout_truncated, stderr_bytes, stderr_truncated = @runtime.captures
      @stdout_capture = Capture.new(bytes: stdout_bytes, truncated: stdout_truncated)
      @stderr_capture = Capture.new(bytes: stderr_bytes, truncated: stderr_truncated)
    end

    # Shared prologue / epilogue + trap-class translator for both
    # invocation verbs. +verb+ is +:eval+ or +:run+; it tags the
    # TrapError message so the failing export is identifiable.
    #
    # The yielded block must return the invocation's raw outcome bytes —
    # i.e. the value of +Runtime#eval+ / +#run+ — which the success path
    # feeds to +Outcome.decode+. Captures and usage are populated by the
    # +ensure+ readouts (#read_usage! / #read_captures!) on every
    # outcome, so +#stdout+ / +#stderr+ / +#usage+ stay readable after a
    # rescued trap.
    # The rescue chain is the single trap-translation boundary —
    # configured-cap paths surface as named TrapError subclasses
    # (+TimeoutError+ / +MemoryLimitError+); everything else surfaces as
    # the base +TrapError+.
    def invoke!(verb)
      begin_invocation!
      return_bytes = yield
      # A Capability Handle in the result is decoded as a Kobako::Handle
      # token; restore it to the host object the guest referenced before
      # handing the value to the Host App. @handler still holds this
      # invocation's table — reset only happens at the next #begin_invocation!.
      # A Handle-free result resolves to itself, so the restoration walk is
      # skipped when the decode carried none.
      value, carried_handle = Codec.track_handles { Outcome.decode(return_bytes) }
      carried_handle ? Codec::HandleWalk.deep_restore(value, @handler) : value
    rescue Kobako::TrapError => e
      raise trap_class_for(e), "Sandbox##{verb} failed: #{e.message}"
    ensure
      read_usage!
      read_captures!
    end
  end
end
