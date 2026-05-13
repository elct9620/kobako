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
    # ({SPEC.md B-01 footnote}[link:../../SPEC.md]).
    DEFAULT_OUTPUT_LIMIT = 1 << 20

    attr_reader :wasm_path, :instance,
                :stdout_buffer, :stderr_buffer,
                :stdout_limit, :stderr_limit, :services

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
    def initialize(wasm_path: nil, stdout_limit: nil, stderr_limit: nil)
      @wasm_path = wasm_path || Kobako::Wasm.default_path
      @stdout_limit = stdout_limit || DEFAULT_OUTPUT_LIMIT
      @stderr_limit = stderr_limit || DEFAULT_OUTPUT_LIMIT
      @instance = Kobako::Wasm::Instance.from_path(@wasm_path)
      @stdout_buffer = OutputBuffer.new(@stdout_limit)
      @stderr_buffer = OutputBuffer.new(@stderr_limit)
      @services = Kobako::Registry.new
      @instance.set_registry(@services)
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
      preamble = @services.guest_preamble
      @instance.setup_wasi_pipes(@stdout_limit, @stderr_limit, preamble, source.b)

      invoke_guest_run
      drain_wasi_output
      outcome_bytes = read_outcome_bytes
      OutcomeDecoder.decode(outcome_bytes)
    end

    private

    # Per-run state reset ({SPEC.md B-03}[link:../../SPEC.md]). Capture
    # buffers and the HandleTable counter are zeroed before the guest runs.
    def reset_run_state!
      @services.reset_handles!
      @stdout_buffer.clear
      @stderr_buffer.clear
    end

    # Drain the WASI stdout/stderr pipes populated during the most recent
    # guest execution into the bounded OutputBuffers
    # ({SPEC.md B-04}[link:../../SPEC.md]). Must be called after
    # `invoke_guest_run` and before the next reset.
    def drain_wasi_output
      stdout_bytes = @instance.take_stdout
      stderr_bytes = @instance.take_stderr
      @stdout_buffer << stdout_bytes unless stdout_bytes.empty?
      @stderr_buffer << stderr_bytes unless stderr_bytes.empty?
    end

    # Invoke `__kobako_run`. Wraps wasmtime / wire errors in TrapError.
    # Source was already delivered via the stdin two-frame protocol in
    # `setup_wasi_pipes` before this call
    # ({SPEC.md ABI Signatures}[link:../../SPEC.md]).
    def invoke_guest_run
      @instance.run
    rescue Kobako::Wasm::Error => e
      raise TrapError, "guest __kobako_run trapped: #{e.message}"
    end

    # Pull the OUTCOME_BUFFER bytes out of guest memory. The +len=0+ case
    # is forwarded to {OutcomeDecoder} as an empty String so a single
    # boundary attributes every wire-violation outcome
    # ({SPEC.md ABI Signatures}[link:../../SPEC.md]).
    def read_outcome_bytes
      ptr, len = Kobako::Wasm.unpack_outcome_ptr_len(@instance.take_outcome)
      @instance.read_memory(ptr, len)
    rescue Kobako::Wasm::Error => e
      raise TrapError, "failed to read OUTCOME_BUFFER: #{e.message}"
    end
  end
end
