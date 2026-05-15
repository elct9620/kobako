# frozen_string_literal: true

require_relative "errors"
require_relative "registry"
require_relative "wire"
require_relative "sandbox/output_buffer"
require_relative "sandbox/outcome_decoder"

module Kobako
  # Kobako::Sandbox — the user-facing entry point for executing guest mruby
  # scripts inside a wasmtime-hosted Wasm module
  # ({SPEC.md B-01}[link:../../SPEC.md]).
  #
  # The Sandbox owns the +Kobako::Wasm::Instance+, the per-instance Registry
  # (which itself owns the per-run HandleTable), and bounded stdout / stderr
  # capture buffers. The underlying wasmtime Engine and compiled Module are
  # cached at process scope by the native ext and never surface to Ruby —
  # constructing many Sandboxes amortises both costs automatically.
  #
  # Buffer overflow policy ({SPEC.md B-04}[link:../../SPEC.md]): once an
  # append would push the cumulative byte count past the per-channel
  # `*_limit`, the OutputBuffer truncates — it stores a prefix that fits
  # under the cap and appends a +[truncated]+ marker on the next read.
  # Truncation does NOT raise. The marker constant lives on
  # +Kobako::Sandbox::OutputBuffer::OUTPUT_TRUNCATION_MARKER+.
  class Sandbox
    # Default per-channel capture ceiling: 1 MiB
    # ({SPEC.md B-01}[link:../../SPEC.md]).
    DEFAULT_OUTPUT_LIMIT = 1 << 20

    # Default wall-clock timeout for a single +#run+: 60 seconds
    # ({SPEC.md B-01}[link:../../SPEC.md]).
    DEFAULT_TIMEOUT_SECONDS = 60.0

    # Default cap on guest linear memory growth: 5 MiB
    # ({SPEC.md B-01}[link:../../SPEC.md]).
    DEFAULT_MEMORY_LIMIT = 5 << 20

    attr_reader :wasm_path, :instance,
                :stdout_buffer, :stderr_buffer,
                :stdout_limit, :stderr_limit,
                :timeout, :memory_limit, :services

    # Returns the complete byte content guest wrote to stdout during the most
    # recent +#run+ as a UTF-8 String, or an empty String before any +#run+
    # call. {SPEC.md B-04}[link:../../SPEC.md]: may contain a +[truncated]+
    # marker when the cap was hit.
    def stdout
      @stdout_buffer.to_s
    end

    # Returns the complete byte content guest wrote to stderr during the most
    # recent +#run+ as a UTF-8 String, or an empty String before any +#run+
    # call. {SPEC.md B-04}[link:../../SPEC.md]: may contain a +[truncated]+
    # marker when the cap was hit.
    def stderr
      @stderr_buffer.to_s
    end

    # Build a fresh Sandbox.
    #
    # +wasm_path+ is the absolute path to the Guest Binary; defaults to the
    # gem-bundled +data/kobako.wasm+. +stdout_limit+ and +stderr_limit+ cap
    # the per-run byte ceiling for each capture channel (default 1 MiB).
    # +timeout+ is the wall-clock cap on a single +#run+ in seconds
    # ({SPEC.md B-01}[link:../../SPEC.md]; default 60.0, +nil+ disables);
    # +memory_limit+ caps guest linear memory growth in bytes
    # ({SPEC.md B-01, E-20}[link:../../SPEC.md]; default 1 MiB, +nil+
    # disables).
    def initialize(wasm_path: nil, stdout_limit: nil, stderr_limit: nil,
                   timeout: DEFAULT_TIMEOUT_SECONDS,
                   memory_limit: DEFAULT_MEMORY_LIMIT)
      @wasm_path = wasm_path || Kobako::Wasm.default_path
      @stdout_limit = stdout_limit || DEFAULT_OUTPUT_LIMIT
      @stderr_limit = stderr_limit || DEFAULT_OUTPUT_LIMIT
      @timeout = normalize_timeout(timeout)
      @memory_limit = normalize_memory_limit(memory_limit)
      @instance = Kobako::Wasm::Instance.from_path(@wasm_path, @timeout, @memory_limit)
      @stdout_buffer = OutputBuffer.new(@stdout_limit)
      @stderr_buffer = OutputBuffer.new(@stderr_limit)
      @services = Kobako::Registry.new
      @instance.registry = @services
    end

    # Declare or retrieve the Service Group named +name+ on this Sandbox
    # ({SPEC.md B-07, B-09, B-10}[link:../../SPEC.md]). +name+ must be a
    # Symbol or String in constant form. Returns the
    # +Kobako::Registry::ServiceGroup+.
    #
    # Raises +ArgumentError+ when called after +#run+, or when +name+ does
    # not match the constant-name pattern.
    def define(name)
      @services.define(name)
    end

    # Execute a guest mruby script
    # ({SPEC.md B-02 / B-03}[link:../../SPEC.md]). +source+ is the mruby
    # source code as a UTF-8 String. Returns the deserialized last
    # expression of the script.
    #
    # Source delivery uses the WASI stdin two-frame protocol
    # ({SPEC.md ABI Signatures}[link:../../SPEC.md]): Frame 1 carries the
    # msgpack-encoded preamble (Service Group registry snapshot) and Frame 2
    # carries the user script UTF-8 bytes. Each frame is prefixed by a
    # 4-byte big-endian u32 length.
    #
    # Raises +Kobako::TrapError+ on a Wasm trap or wire-violation fallback;
    # +Kobako::SandboxError+ when the guest ran to completion but failed;
    # +Kobako::ServiceError+ on an unrescued Service capability failure.
    def run(source)
      raise SandboxError, "source must be a String, got #{source.class}" unless source.is_a?(String)

      @services.seal!
      reset_run_state!

      invoke_guest_run(@services.guest_preamble, source.b)
      drain_captured_output
      take_result!
    end

    private

    # Coerce +timeout+ into the Float seconds the ext expects, or +nil+ to
    # mean the cap is disabled ({SPEC.md B-01}[link:../../SPEC.md]). Any
    # finite non-positive value is rejected — a zero or negative timeout
    # would either fire instantly or never, both of which would surprise
    # callers more than an early +ArgumentError+.
    def normalize_timeout(timeout)
      return nil if timeout.nil?
      raise ArgumentError, "timeout must be Numeric or nil, got #{timeout.class}" unless timeout.is_a?(Numeric)

      seconds = timeout.to_f
      raise ArgumentError, "timeout must be > 0 (got #{timeout})" unless seconds.positive? && seconds.finite?

      seconds
    end

    # Coerce +memory_limit+ into the byte cap the ext expects, or +nil+ to
    # mean unbounded ({SPEC.md B-01, E-20}[link:../../SPEC.md]). Must be a
    # positive Integer when set; +Float+ or zero/negative values are
    # rejected.
    def normalize_memory_limit(memory_limit)
      return nil if memory_limit.nil?
      unless memory_limit.is_a?(Integer) && memory_limit.positive?
        raise ArgumentError, "memory_limit must be a positive Integer or nil, got #{memory_limit.inspect}"
      end

      memory_limit
    end

    # Per-run state reset ({SPEC.md B-03}[link:../../SPEC.md]). Capture
    # buffers and the HandleTable counter are zeroed before the guest runs.
    def reset_run_state!
      @services.reset_handles!
      @stdout_buffer.clear
      @stderr_buffer.clear
    end

    # Append the WASI stdout/stderr bytes captured during the most recent
    # guest execution into the bounded OutputBuffers
    # ({SPEC.md B-04}[link:../../SPEC.md]). Must be called after
    # `invoke_guest_run` and before the next reset. The per-channel cap is
    # enforced inside +OutputBuffer+; the underlying WASI transport pipes
    # are uncapped because SPEC.md B-04 requires truncation be a non-error
    # outcome.
    def drain_captured_output
      stdout_bytes = @instance.stdout
      stderr_bytes = @instance.stderr
      @stdout_buffer << stdout_bytes unless stdout_bytes.empty?
      @stderr_buffer << stderr_bytes unless stderr_bytes.empty?
    end

    # Drive +Instance#run+ with the two stdin frames (preamble + source).
    # Wraps wasmtime / wire errors in TrapError so the Sandbox layer maps
    # cleanly to the three-class taxonomy. The configured-cap paths
    # (SPEC.md E-19 / E-20) are routed to the named TrapError subclasses
    # so callers that want to surface a specific reason can rescue them;
    # everything else falls through to the base TrapError.
    def invoke_guest_run(preamble, source)
      @instance.run(preamble, source)
    rescue Kobako::Wasm::TimeoutError => e
      raise TimeoutError, "guest exceeded timeout: #{e.message}"
    rescue Kobako::Wasm::MemoryLimitError => e
      raise MemoryLimitError, "guest exceeded memory_limit: #{e.message}"
    rescue Kobako::Wasm::Error => e
      raise TrapError, "guest __kobako_run trapped: #{e.message}"
    end

    # Take OUTCOME_BUFFER bytes from guest memory via +Instance#outcome!+
    # and decode them into the Sandbox-level result — the unwrapped mruby
    # return value, or a raised three-layer
    # ({SPEC.md "Error Scenarios"}[link:../../SPEC.md]) exception. A zero-
    # length outcome is delivered to {OutcomeDecoder} as an empty String so a
    # single boundary attributes every wire-violation outcome
    # ({SPEC.md ABI Signatures}[link:../../SPEC.md]).
    #
    # The bang reflects the destructive ext call beneath: the underlying
    # +__kobako_take_outcome+ export invalidates the buffer pointer, so this
    # method must be called at most once per +#run+.
    def take_result!
      OutcomeDecoder.decode(@instance.outcome!)
    rescue Kobako::Wasm::Error => e
      raise TrapError, "failed to read OUTCOME_BUFFER: #{e.message}"
    end
  end
end
