# frozen_string_literal: true

require "forwardable"

require_relative "capture"
require_relative "errors"
require_relative "invocation"
require_relative "outcome"
require_relative "rpc/server"
require_relative "rpc/envelope"
require_relative "sandbox_options"
require_relative "snippet"

module Kobako
  # Kobako::Sandbox — the user-facing entry point for executing guest mruby
  # scripts inside a wasmtime-hosted Wasm module
  # ({docs/behavior.md B-01}[link:../../docs/behavior.md]).
  #
  # The Sandbox owns the +Kobako::Wasm::Instance+, the per-instance RPC Server
  # (which itself owns the per-run HandleTable), and the per-channel byte
  # caches for guest stdout / stderr capture. The underlying wasmtime Engine
  # and compiled Module are cached at process scope by the native ext and
  # never surface to Ruby — constructing many Sandboxes amortises both costs
  # automatically.
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

    attr_reader :wasm_path, :instance,
                :options,
                :services, :snippets

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
      @wasm_path = wasm_path || Kobako::Wasm.default_path
      @options = SandboxOptions.new(timeout: timeout, memory_limit: memory_limit, stdout_limit: stdout_limit,
                                    stderr_limit: stderr_limit)
      @services = Kobako::RPC::Server.new
      @snippets = Snippet::Table.new
      @instance = Kobako::Wasm::Instance.from_path(@wasm_path, @options.timeout, @options.memory_limit,
                                                   @options.stdout_limit, @options.stderr_limit)
      @instance.server = @services
      clear_captures!
    end

    # Declare or retrieve the Namespace named +name+ on this Sandbox
    # ({docs/behavior.md B-07, B-09, B-10}[link:../../docs/behavior.md]). +name+ must be a
    # Symbol or String in constant form. Returns the +Kobako::RPC::Namespace+.
    #
    # Raises +ArgumentError+ when called after the first invocation, or
    # when +name+ does not match the constant-name pattern.
    def define(name)
      @services.define(name)
    end

    # Register a source snippet on this Sandbox
    # ({docs/behavior.md B-32}[link:../../docs/behavior.md]). Subsequent
    # invocations (+#eval+ or +#run+) replay the snippet against the fresh
    # +mrb_state+ before per-invocation source / entrypoint resolution; the
    # +name+ becomes the +(snippet:Name)+ backtrace filename. Delegates
    # name / duplicate validation to +Kobako::Snippet::Table+.
    #
    # Returns +self+ to allow chaining.
    #
    # Raises +ArgumentError+ when +name+ does not match the constant
    # pattern ({docs/behavior.md E-34}[link:../../docs/behavior.md]), when +name+
    # duplicates an already-registered snippet
    # ({docs/behavior.md E-33}[link:../../docs/behavior.md]), or when called
    # after the first invocation
    # ({docs/behavior.md E-35, B-33}[link:../../docs/behavior.md]).
    def preload(code:, name:)
      raise ArgumentError, "cannot preload after first Sandbox invocation" if @services.sealed?
      raise ArgumentError, "code must be a String, got #{code.class}" unless code.is_a?(String)

      @snippets.register(code, name)
      self
    end

    # Dispatch into a preloaded entrypoint constant
    # ({docs/behavior.md B-31}[link:../../docs/behavior.md]). Delegates host
    # pre-flight (E-24 / E-25 / E-29 / E-30) and wire encoding to
    # +Kobako::Invocation+ / +Kobako::Invocation#encode+; the guest
    # resolves +target+ as a top-level constant, calls +#call+ on it
    # with +args+ / +kwargs+, and returns the deserialized result. The
    # first invocation seals the Service registry and snippet table
    # (B-07 / B-33). Runtime errors follow the same three-class taxonomy
    # as +#eval+.
    def run(target, *args, **kwargs)
      invocation = Invocation.new(entrypoint: target, args: args, kwargs: kwargs)
      invoke!(:run) do
        @instance.run(@services.encoded_preamble, @snippets.encode, invocation.encode)
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
        @instance.eval(@services.encoded_preamble, code.b, @snippets.encode)
      end
    end

    private

    # Per-invocation prologue ({docs/behavior.md B-03 / B-07 /
    # B-33}[link:../../docs/behavior.md]). Seals the Service / snippet
    # registries on first call (idempotent) and zeros the per-invocation
    # capability state — capture buffers, truncation predicates, and the
    # HandleTable counter — before the guest runs.
    def begin_invocation!
      @services.seal!
      @services.reset_handles!
      clear_captures!
    end

    # Reset both per-channel captures to the pre-invocation sentinel
    # ({docs/behavior.md B-05}[link:../../docs/behavior.md]). Shared by +#initialize+
    # (first-time setup) and +#begin_invocation!+ (between-invocation
    # reset) so both paths agree on what "empty capture" means.
    def clear_captures!
      @stdout_capture = Capture::EMPTY
      @stderr_capture = Capture::EMPTY
    end

    # Read the per-channel capture pairs (+[bytes, truncated]+) from the
    # ext after an invocation completes and wrap each as a +Capture+ value
    # object. The ext clips +bytes+ to the configured cap and sets
    # +truncated+ when the guest produced strictly more than +cap+ bytes
    # ({docs/behavior.md B-04}[link:../../docs/behavior.md]). Mirror of {#clear_captures!}
    # at the post-invocation boundary.
    def read_captures!
      out_bytes, out_truncated = @instance.stdout
      err_bytes, err_truncated = @instance.stderr
      @stdout_capture = Capture.from_ext(out_bytes, out_truncated)
      @stderr_capture = Capture.from_ext(err_bytes, err_truncated)
    end

    # Shared prologue / epilogue + trap-class translator for both
    # invocation verbs. +verb+ is +:eval+ or +:run+; it tags the
    # TrapError message so the failing export is identifiable. The
    # rescue chain is the single trap-translation boundary — wasmtime /
    # wire failures from the guest call and from the subsequent
    # +Instance#outcome!+ read both flow through here, so an
    # OUTCOME_BUFFER read failure attributes to the same export name as
    # the guest call itself. Configured-cap paths
    # ({docs/behavior.md E-19 / E-20}[link:../../docs/behavior.md]) surface as
    # named TrapError subclasses.
    def invoke!(verb)
      begin_invocation!
      yield
      read_captures!
      Outcome.decode(@instance.outcome!)
    rescue Kobako::Wasm::TimeoutError => e
      raise TimeoutError, "guest exceeded timeout: #{e.message}"
    rescue Kobako::Wasm::MemoryLimitError => e
      raise MemoryLimitError, "guest exceeded memory_limit: #{e.message}"
    rescue Kobako::Wasm::Error => e
      raise TrapError, "guest __kobako_#{verb} trapped: #{e.message}"
    end
  end
end
