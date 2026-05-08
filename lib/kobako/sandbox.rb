# frozen_string_literal: true

require_relative "errors"
require_relative "registry"
require_relative "wire/envelope"
require_relative "wire/error"

module Kobako
  # Kobako::Sandbox — the user-facing entry point for executing guest mruby
  # scripts inside a wasmtime-hosted Wasm module (SPEC.md §B-01).
  #
  # The Sandbox owns the wasmtime pipeline (Engine / Module / Store /
  # Instance), the per-instance Registry (which itself owns the per-run
  # HandleTable), and bounded stdout / stderr capture buffers.
  #
  # Engine reuse note: the constructor accepts an optional `engine:` argument
  # so test setups can build many Sandboxes against one shared Engine; a
  # production caller running many runs should also share the Engine,
  # because Engine creation is comparatively expensive.
  #
  # Buffer overflow policy (SPEC.md §B-04): once an append would push the
  # cumulative byte count past the per-channel `*_limit`, the OutputBuffer
  # truncates — it stores a prefix that fits under the cap and appends a
  # `[truncated]` marker on the next read. Truncation does NOT raise.
  # rubocop:disable Metrics/ClassLength
  class Sandbox
    # Default per-channel capture ceiling: 1 MiB (SPEC.md §B-01 footnote).
    DEFAULT_OUTPUT_LIMIT = 1 << 20

    # Marker appended to a buffer that hit its capture limit (SPEC.md §B-04).
    OUTPUT_TRUNCATION_MARKER = "[truncated]"

    # In-memory bounded byte buffer for one of the guest's output channels.
    # Tracks accumulated bytes (binary-encoded) and enforces the per-channel
    # cap by truncating-with-marker (SPEC.md §B-04).
    class OutputBuffer
      attr_reader :limit

      def initialize(limit)
        raise ArgumentError, "limit must be a positive Integer" unless limit.is_a?(Integer) && limit.positive?

        @limit = limit
        @bytes = String.new(encoding: Encoding::ASCII_8BIT)
        @truncated = false
      end

      # Append +bytes+ to the buffer. If the append would push the
      # cumulative byte count past the limit, the buffer keeps as many
      # leading bytes as fit and seals itself; subsequent appends are
      # discarded. SPEC.md §B-04 — truncation is a non-error outcome.
      def <<(bytes)
        return self if @truncated

        appended = bytes.to_s.b
        room = @limit - @bytes.bytesize
        if appended.bytesize <= room
          @bytes << appended
        else
          @bytes << appended.byteslice(0, room) if room.positive?
          @truncated = true
        end
        self
      end

      # @return [Boolean] whether the buffer was sealed by an overflow.
      def truncated?
        @truncated
      end

      # @return [Integer] number of bytes currently stored.
      def bytesize
        @bytes.bytesize
      end

      # @return [Boolean] whether the buffer is empty.
      def empty?
        @bytes.empty?
      end

      # @return [String] accumulated bytes as a UTF-8 String, with the
      #   `[truncated]` marker appended when the buffer overflowed.
      def to_s
        copy = @bytes.dup
        copy << OUTPUT_TRUNCATION_MARKER.b if @truncated
        copy.force_encoding(Encoding::UTF_8)
        copy.valid_encoding? ? copy : copy.dup.force_encoding(Encoding::ASCII_8BIT)
      end

      # Reset the buffer to empty. Used at the per-`#run` boundary.
      def clear
        @bytes.clear
        @truncated = false
        self
      end
    end

    attr_reader :wasm_path, :engine, :module_, :store, :instance,
                :stdout_buffer, :stderr_buffer,
                :stdout_limit, :stderr_limit, :services

    # Return the complete byte content guest wrote to stdout during the most
    # recent `#run`. Returns an empty String before any `#run` call.
    # SPEC.md §B-04: may contain a `[truncated]` marker when the cap was hit.
    #
    # @return [String] UTF-8 encoded stdout capture.
    def stdout
      @stdout_buffer.to_s
    end

    # Return the complete byte content guest wrote to stderr during the most
    # recent `#run`. Returns an empty String before any `#run` call.
    # SPEC.md §B-04: may contain a `[truncated]` marker when the cap was hit.
    #
    # @return [String] UTF-8 encoded stderr capture.
    def stderr
      @stderr_buffer.to_s
    end

    # Build a fresh Sandbox.
    #
    # @param wasm_path [String, nil] absolute path to the Guest Binary.
    # @param stdout_limit [Integer, nil] per-run stdout byte ceiling.
    # @param stderr_limit [Integer, nil] per-run stderr byte ceiling.
    # @param engine [Kobako::Wasm::Engine, nil] optional shared Engine.
    def initialize(wasm_path: nil, stdout_limit: nil, stderr_limit: nil, engine: nil)
      @wasm_path = wasm_path || Kobako::Wasm.default_path
      @stdout_limit = stdout_limit || DEFAULT_OUTPUT_LIMIT
      @stderr_limit = stderr_limit || DEFAULT_OUTPUT_LIMIT
      build_wasm_pipeline(engine)
      @stdout_buffer = OutputBuffer.new(@stdout_limit)
      @stderr_buffer = OutputBuffer.new(@stderr_limit)
      @services = Kobako::Registry.new
      @instance.set_registry(@services)
    end

    # The HandleTable owned by the Sandbox's Registry. Exposed for tests
    # and integration with the wire-layer Handle wrapping path.
    #
    # @return [Kobako::Registry::HandleTable]
    def handle_table
      @services.handle_table
    end

    # Declare or retrieve a Service Group on this Sandbox (SPEC.md §B-07,
    # B-09, B-10).
    #
    # @param name [Symbol, String] constant-form group name.
    # @return [Kobako::Registry::ServiceGroup]
    # @raise [ArgumentError] when called after `#run`, or when the name
    #   does not match the constant-name pattern.
    def define(name)
      @services.define(name)
    end

    # Execute a guest mruby script (SPEC.md §B-02 / §B-03).
    #
    # Source delivery uses the WASI stdin two-frame protocol (SPEC.md
    # §ABI Signatures): Frame 1 carries the msgpack-encoded preamble (Service
    # Group registry snapshot) and Frame 2 carries the user script UTF-8
    # bytes. Each frame is prefixed by a 4-byte big-endian u32 length.
    #
    # @param source [String] mruby source code (UTF-8).
    # @return [Object] the deserialized last expression of the script.
    # @raise [Kobako::TrapError]    Wasm trap or wire-violation fallback.
    # @raise [Kobako::SandboxError] guest ran to completion but failed.
    # @raise [Kobako::ServiceError] unrescued Service capability failure.
    def run(source)
      raise SandboxError, "source must be a String, got #{source.class}" unless source.is_a?(String)

      @services.seal!
      reset_run_state!
      preamble = @services.guest_preamble
      @instance.setup_wasi_pipes(@stdout_limit, @stderr_limit, preamble, source.b)

      invoke_guest_run
      drain_wasi_output
      outcome_bytes = read_outcome_bytes
      decode_outcome(outcome_bytes)
    end

    private

    # Per-run state reset (SPEC.md §B-03). Capture buffers and the
    # HandleTable counter are zeroed before the guest runs.
    def reset_run_state!
      @services.reset_handles!
      @stdout_buffer.clear
      @stderr_buffer.clear
    end

    # Drain the WASI stdout/stderr pipes populated during the most recent
    # guest execution into the bounded OutputBuffers (SPEC.md §B-04).
    # Must be called after `invoke_guest_run` and before the next reset.
    def drain_wasi_output
      stdout_bytes = @instance.take_stdout
      stderr_bytes = @instance.take_stderr
      @stdout_buffer << stdout_bytes unless stdout_bytes.empty?
      @stderr_buffer << stderr_bytes unless stderr_bytes.empty?
    end

    # Invoke `__kobako_run`. Wraps wasmtime / wire errors in TrapError.
    # Source was already delivered via the stdin two-frame protocol in
    # `setup_wasi_pipes` before this call (SPEC.md §ABI Signatures).
    def invoke_guest_run
      @instance.run
    rescue Kobako::Wasm::Error => e
      raise TrapError, "guest __kobako_run trapped: #{e.message}"
    end

    # Pull the OUTCOME_BUFFER bytes out of guest memory.
    def read_outcome_bytes
      packed = @instance.take_outcome
      ptr, len = unpack_packed_u64(packed)
      raise TrapError, "guest exited without writing an outcome (len=0)" if len.zero?

      @instance.read_memory(ptr, len)
    rescue Kobako::Wasm::Error => e
      raise TrapError, "failed to read OUTCOME_BUFFER: #{e.message}"
    end

    def unpack_packed_u64(packed)
      ptr = (packed >> 32) & 0xffff_ffff
      len = packed & 0xffff_ffff
      [ptr, len]
    end

    # Three-layer error attribution (SPEC.md §"Error Scenarios"):
    #
    #   * tag 0x01, decode OK                 → return Result.value
    #   * tag 0x01, decode fails              → SandboxError (E-09)
    #   * tag 0x02, origin="service"          → ServiceError (E-13)
    #   * tag 0x02, origin="sandbox"/missing  → SandboxError (E-04..E-07)
    #   * tag 0x02, decode fails              → SandboxError (E-08)
    #   * unknown tag                         → TrapError    (E-03)
    def decode_outcome(bytes)
      tag, body = split_outcome_tag(bytes)
      case tag
      when Kobako::Wire::Envelope::OUTCOME_TAG_RESULT
        decode_outcome_result(body)
      when Kobako::Wire::Envelope::OUTCOME_TAG_PANIC
        raise decode_outcome_panic(body)
      else
        raise TrapError, format("unknown outcome tag 0x%<tag>02x", tag: tag)
      end
    end

    def split_outcome_tag(bytes)
      bytes = bytes.b
      [bytes.getbyte(0), bytes.byteslice(1, bytes.bytesize - 1)]
    end

    # Decode failure on a known Result tag is a SandboxError (E-09): the
    # framing was fine, but the wrapped value is unrepresentable.
    def decode_outcome_result(body)
      Kobako::Wire::Envelope.decode_result(body).value
    rescue Kobako::Wire::Error => e
      raise wire_violation_error(SandboxError, "result envelope decode failed: #{e.message}")
    end

    # Decode failure on a known Panic tag is a SandboxError (E-08).
    def decode_outcome_panic(body)
      build_panic_error(Kobako::Wire::Envelope.decode_panic(body))
    rescue Kobako::Wire::Error => e
      wire_violation_error(SandboxError, "panic envelope decode failed: #{e.message}")
    end

    # Map a decoded Panic envelope into the corresponding three-layer
    # Ruby exception. `origin == "service"` → ServiceError; everything
    # else → SandboxError.
    def build_panic_error(panic)
      target_class = panic.origin == Kobako::Wire::Envelope::Panic::ORIGIN_SERVICE ? ServiceError : SandboxError
      target_class.new(
        panic.message,
        origin: panic.origin,
        klass: panic.klass,
        backtrace_lines: panic.backtrace,
        details: panic.details
      )
    end

    def wire_violation_error(klass, message)
      klass.new(
        message,
        origin: Kobako::Wire::Envelope::Panic::ORIGIN_SANDBOX,
        klass: "Kobako::WireError"
      )
    end

    def build_wasm_pipeline(engine)
      @engine = engine || Kobako::Wasm::Engine.new
      @module_ = Kobako::Wasm::Module.from_file(@engine, @wasm_path)
      @store = Kobako::Wasm::Store.new(@engine)
      @instance = Kobako::Wasm::Instance.new(@engine, @module_, @store)
    end
  end
  # rubocop:enable Metrics/ClassLength
end
