# frozen_string_literal: true

require "forwardable"

require_relative "capture"
require_relative "catalog/snippet"
require_relative "errors"
require_relative "catalog/handler"
require_relative "transport/run"
require_relative "outcome"
require_relative "catalog/binding"
require_relative "transport/dispatcher"
require_relative "transport/request"
require_relative "transport/response"
require_relative "sandbox_options"
require_relative "usage"

module Kobako
  # Kobako::Sandbox — the user-facing entry point for executing guest mruby
  # scripts inside a wasmtime-hosted Wasm module
  # ({docs/behavior.md B-01}[link:../../docs/behavior.md]).
  #
  # The Sandbox owns the +Kobako::Runtime+, the per-Sandbox
  # +Kobako::Catalog::Handler+ ({docs/behavior.md B-19}[link:../../docs/behavior.md]),
  # the per-instance +Kobako::Catalog::Binding+ (which receives the
  # +Catalog::Handler+ by injection so guest→host dispatch and host→guest
  # auto-wrap share one allocator), and the dispatch +Proc+ /
  # +yield_to_guest+ lambda installed on the Runtime via
  # +Runtime#on_dispatch=+ ({docs/behavior.md B-12}[link:../../docs/behavior.md]).
  # The underlying wasmtime Engine and compiled Module are cached at process
  # scope by the native ext and never surface to Ruby — constructing many
  # Sandboxes amortises both costs automatically.
  #
  # Output capture policy ({docs/behavior.md B-04}[link:../../docs/behavior.md]): the
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

    # Per-cap accessors forward to the immutable +SandboxOptions+ Value
    # Object so the Host App still reads them off Sandbox directly.
    def_delegators :@options, :timeout, :memory_limit, :stdout_limit, :stderr_limit

    # Returns the bytes the guest wrote to stdout during the most recent
    # invocation as a UTF-8 String, clipped at +stdout_limit+. Empty before
    # any invocation. {docs/behavior.md B-04}[link:../../docs/behavior.md] — the byte
    # content never contains a truncation sentinel; use +#stdout_truncated?+ to
    # observe overflow.
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
    # exceeded +stdout_limit+ ({docs/behavior.md B-04}[link:../../docs/behavior.md]). Resets
    # to +false+ at the start of the next invocation
    # ({docs/behavior.md B-03}[link:../../docs/behavior.md]).
    def stdout_truncated?
      @stdout_capture.truncated?
    end

    # Returns +true+ iff stderr capture during the most recent invocation
    # exceeded +stderr_limit+. Mirror of +#stdout_truncated?+.
    def stderr_truncated?
      @stderr_capture.truncated?
    end

    # Returns the +Kobako::Usage+ value object for the most recent
    # invocation ({docs/behavior.md B-35}[link:../../docs/behavior.md]).
    # Carries +wall_time+ (Float seconds the guest export call spent
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
    # gem-bundled +data/kobako.wasm+. The four caps (+stdout_limit+,
    # +stderr_limit+, +timeout+, +memory_limit+) are forwarded verbatim to
    # +Kobako::SandboxOptions+, which owns their DEFAULT fallback and
    # normalisation. The constructed +SandboxOptions+ is exposed as
    # +#options+ and the four caps remain readable directly on Sandbox via
    # +Forwardable+ delegation.
    def initialize(wasm_path: nil, stdout_limit: nil, stderr_limit: nil,
                   timeout: SandboxOptions::DEFAULT_TIMEOUT_SECONDS,
                   memory_limit: SandboxOptions::DEFAULT_MEMORY_LIMIT)
      @wasm_path = wasm_path || Kobako::Runtime.default_path
      @options = SandboxOptions.new(timeout: timeout, memory_limit: memory_limit, stdout_limit: stdout_limit,
                                    stderr_limit: stderr_limit)
      @handler = Catalog::Handler.new
      @services = Kobako::Catalog::Binding.new(handler: @handler)
      @snippets = Catalog::Snippet::Table.new
      @runtime = Kobako::Runtime.from_path(@wasm_path, @options.timeout, @options.memory_limit,
                                           @options.stdout_limit, @options.stderr_limit)
      install_dispatch_proc!
      reset_invocation_state!
    end

    # Declare or retrieve the Namespace named +name+ on this Sandbox
    # ({docs/behavior.md B-07, B-09, B-10}[link:../../docs/behavior.md]). +name+ must be a
    # Symbol or String in constant form. Returns the
    # +Kobako::Namespace+.
    #
    # Raises +ArgumentError+ when called after the first invocation, or
    # when +name+ does not match the constant-name pattern.
    def define(name)
      @services.define(name)
    end

    # Register a snippet on this Sandbox in one of two forms
    # ({docs/behavior.md B-32}[link:../../docs/behavior.md]):
    #
    #   * +preload(code: source, name: Name)+ — +source+ is mruby source
    #     as a +String+ and +Name+ matches +/\A[A-Z]\w*\z/+. The +name+
    #     becomes the snippet's +(snippet:Name)+ backtrace filename and
    #     is the dedupe key for E-33.
    #   * +preload(binary: bytes)+ — +bytes+ is precompiled RITE
    #     bytecode as a +String+. The canonical name, when present,
    #     lives in the bytecode's embedded +debug_info+ and is resolved
    #     by the guest at load time; the host treats the bytes as
    #     opaque. Structural failures
    #     ({docs/behavior.md E-37 / E-38}[link:../../docs/behavior.md])
    #     surface as +Kobako::BytecodeError+ on the first invocation.
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
    # pattern ({docs/behavior.md E-34}[link:../../docs/behavior.md]),
    # when +name+ duplicates an already-registered +code:+ form snippet
    # ({docs/behavior.md E-33}[link:../../docs/behavior.md]), or when
    # called after the first invocation
    # ({docs/behavior.md E-35, B-33}[link:../../docs/behavior.md]).
    def preload(code: nil, name: nil, binary: nil)
      raise ArgumentError, "cannot preload after first Sandbox invocation" if @services.sealed?

      @snippets.register(code: code, name: name, binary: binary)
      self
    end

    # Dispatch into a preloaded entrypoint constant
    # ({docs/behavior.md B-31}[link:../../docs/behavior.md]). Delegates host
    # pre-flight (E-24 / E-25 / E-29 / E-30) and wire encoding to
    # +Kobako::Transport::Run+ / +Kobako::Transport::Run#encode+; the guest
    # resolves +target+ as a top-level constant, calls +#call+ on it
    # with +args+ / +kwargs+, and returns the deserialized result. The
    # first invocation seals the Service registry and snippet table
    # (B-07 / B-33). Runtime errors follow the same three-class taxonomy
    # as +#eval+.
    def run(target, *args, **kwargs)
      run_envelope = Transport::Run.new(entrypoint: target, args: args, kwargs: kwargs)
      invoke!(:run) do
        @runtime.run(@services.encoded_preamble, @snippets.encode, run_envelope.encode(@handler))
      end
    end

    # Execute a guest mruby source string in a fresh +mrb_state+
    # ({docs/behavior.md B-02 / B-03 / B-06}[link:../../docs/behavior.md]). +code+ is the
    # mruby source as a UTF-8 String. Returns the deserialized last
    # expression of the source.
    #
    # Source delivery uses the WASI stdin three-frame protocol
    # ({docs/wire-codec.md Invocation channels}[link:../../docs/wire-codec.md]):
    # Frame 1 carries the msgpack-encoded preamble (Namespace / Member
    # registry snapshot), Frame 2 carries the user source UTF-8 bytes, and
    # Frame 3 carries the snippet table registered via +#preload+ (B-32).
    # Each frame is prefixed by a 4-byte big-endian u32 length; Frame 3 is
    # mandatory-presence — an empty snippet table sends an empty msgpack
    # array, never an absent frame.
    #
    # The first invocation seals the Service registry and snippet table
    # ({docs/behavior.md B-07 / B-33}[link:../../docs/behavior.md]); subsequent
    # +#define+ / +#preload+ calls raise +ArgumentError+.
    #
    # Raises +Kobako::TrapError+ on a Wasm trap or wire-violation fallback;
    # +Kobako::SandboxError+ when the guest ran to completion but failed
    # (including when +code+ is +nil+ or not a String, or when a preloaded
    # snippet's replay raises — E-36);
    # +Kobako::ServiceError+ on an unrescued Service capability failure.
    def eval(code)
      raise SandboxError, "code must be a String, got #{code.class}" unless code.is_a?(String)

      invoke!(:eval) do
        @runtime.eval(@services.encoded_preamble, code.b, @snippets.encode)
      end
    end

    private

    # Configure the +Runtime+'s host↔guest dispatch wiring
    # ({docs/behavior.md B-12}[link:../../docs/behavior.md]). Builds a
    # lambda that re-enters the guest via
    # +Runtime#yield_to_active_invocation+ (B-24) and a dispatch +Proc+
    # that routes guest→host calls through the stateless
    # +Transport::Dispatcher+, capturing +@services+ / +@handler+ in the
    # closure. Both are registered on the +Runtime+ once at construction
    # time so the wasm ext callback can fire without further setup.
    def install_dispatch_proc!
      yield_to_guest = ->(args_bytes) { @runtime.yield_to_active_invocation(args_bytes) }
      @runtime.on_dispatch = lambda do |request_bytes|
        Transport::Dispatcher.dispatch(request_bytes, @services, @handler, yield_to_guest)
      end
    end

    # Per-invocation prologue ({docs/behavior.md B-03 / B-07 /
    # B-33}[link:../../docs/behavior.md]). Seals the Service / snippet
    # registries on first call (idempotent) and zeros the per-invocation
    # capability state — capture buffers, truncation predicates, and the
    # +Catalog::Handler+ counter — before the guest runs. The
    # +Catalog::Handler+ itself is held as +@handler+ and never exposed beyond
    # this class: SPEC.md Terminology pins it as "Not exposed to the
    # Host App" (B-19 / B-20 / E-29).
    def begin_invocation!
      @services.seal!
      @handler.reset!
      reset_invocation_state!
    end

    # Reset all per-invocation observable state to its pre-invocation
    # sentinels — both per-channel captures
    # ({docs/behavior.md B-05}[link:../../docs/behavior.md]) and the
    # per-last-invocation usage record
    # ({docs/behavior.md B-35}[link:../../docs/behavior.md]). Shared by
    # +#initialize+ (first-time setup) and +#begin_invocation!+
    # (between-invocation reset) so both paths agree on what
    # "pre-invocation state" means.
    def reset_invocation_state!
      @stdout_capture = Capture::EMPTY
      @stderr_capture = Capture::EMPTY
      @usage = Usage::EMPTY
    end

    # Read the per-last-invocation +wall_time+ and +memory_peak+ from
    # the ext and wrap them as a +Kobako::Usage+ value object
    # ({docs/behavior.md B-35}[link:../../docs/behavior.md]). Runs in
    # the +invoke!+ +ensure+ block so the usage record is populated on
    # every outcome — value return, +Kobako::TrapError+ (including
    # +TimeoutError+ / +MemoryLimitError+), +Kobako::SandboxError+,
    # and +Kobako::ServiceError+. On the success path the same figures
    # already arrive via +Snapshot#usage+; on the trap path the Snapshot
    # never reaches Ruby so the ext readout here is the only source.
    #
    # The ext returns a positional 2-tuple +[wall_time, memory_peak]+
    # whose order matches the +Kobako::Usage+ field order; the
    # destructure-then-kwargs handoff below is the explicit
    # positional→keyword conversion point.
    def read_usage!
      wall_time, memory_peak = @runtime.usage
      @usage = Usage.new(wall_time: wall_time, memory_peak: memory_peak)
    end

    # Pick the +TrapError+ subclass to re-raise based on +err+'s actual
    # class. Cap-trap subclasses
    # ({docs/behavior.md E-19 / E-20}[link:../../docs/behavior.md])
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

    # Shared prologue / epilogue + trap-class translator for both
    # invocation verbs. +verb+ is +:eval+ or +:run+; it tags the
    # TrapError message so the failing export is identifiable.
    #
    # The yielded block must return a +Kobako::Snapshot+ — i.e. the
    # value of +Runtime#eval+ / +#run+ (SPEC.md Internal Concepts →
    # Snapshot). The success path unpacks every observable from the
    # Snapshot in one go: +#stdout+ / +#stderr+ pack into +Capture+,
    # +#usage+ packs into +Usage+, +#return_bytes+ feeds +Outcome.decode+.
    # The rescue chain is the single trap-translation boundary —
    # configured-cap paths
    # ({docs/behavior.md E-19 / E-20}[link:../../docs/behavior.md])
    # surface as named TrapError subclasses; everything else surfaces as
    # the base +TrapError+.
    def invoke!(verb)
      begin_invocation!
      snapshot = yield
      @stdout_capture = snapshot.stdout
      @stderr_capture = snapshot.stderr
      Outcome.decode(snapshot.return_bytes)
    rescue Kobako::TrapError => e
      raise trap_class_for(e), "Sandbox##{verb} failed: #{e.message}"
    ensure
      read_usage!
    end
  end
end
