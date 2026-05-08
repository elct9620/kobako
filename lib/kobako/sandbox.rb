# frozen_string_literal: true

require_relative "handle_table"
require_relative "service"
require_relative "wire/envelope"
require_relative "wire/error"

module Kobako
  # Kobako::Sandbox — the user-facing entry point for executing guest mruby
  # scripts inside a wasmtime-hosted Wasm module (SPEC §B-01).
  #
  # This file delivers item #14 from the implementation roadmap: the
  # constructor, owned wasmtime wiring (Engine / Module / Store / Instance),
  # per-instance HandleTable, and bounded stdout / stderr capture buffers.
  # The actual `#run` execution path is item #16; this class raises
  # NotImplementedError if `#run` is called before that lands. The Service
  # Registry is item #15; for now `#services` returns a placeholder that
  # raises on `#define`.
  #
  # Engine reuse note: the constructor accepts an optional `engine:` argument
  # so test setups can build many Sandboxes against one shared Engine; a
  # production caller running many runs should also share the Engine,
  # because Engine creation is comparatively expensive. Constructing one
  # Engine per Sandbox is fine for ad-hoc scripts and tests but wasteful in
  # a hot loop.
  #
  # Buffer overflow policy (placeholder): per task #14 instructions the
  # OutputBuffer raises `Kobako::Sandbox::OutputLimitExceeded` when an
  # append would exceed the configured limit. SPEC §B-04 specifies that the
  # final user-visible behavior is truncation with a `[truncated]` marker;
  # the truncation policy will be applied at the capture boundary in item
  # #16 (the run path) by catching this error and finalizing the buffer.
  # Item #20 will rewire OutputLimitExceeded into the canonical
  # `Kobako::SandboxError` hierarchy.
  class Sandbox
    # Default per-channel capture ceiling: 1 MiB (SPEC §B-01 footnote).
    DEFAULT_OUTPUT_LIMIT = 1 << 20

    # Raised by OutputBuffer#<< when an append would exceed the buffer's
    # configured limit. Placeholder — item #20 rewires raise sites into the
    # canonical Kobako::SandboxError hierarchy.
    class OutputLimitExceeded < StandardError; end

    # In-memory bounded byte buffer for one of the guest's output channels.
    # Tracks accumulated bytes (binary-encoded), enforces a hard cap, and
    # exposes #to_s / #clear for the host. `<<` raises OutputLimitExceeded
    # when an append would push the byte count past the limit.
    class OutputBuffer
      attr_reader :limit

      def initialize(limit)
        raise ArgumentError, "limit must be a positive Integer" unless limit.is_a?(Integer) && limit.positive?

        @limit = limit
        @bytes = String.new(encoding: Encoding::ASCII_8BIT)
      end

      # Append +bytes+ to the buffer. Raises OutputLimitExceeded if doing so
      # would push the cumulative byte count above the limit; the buffer is
      # left unchanged when this happens (atomic-on-error).
      def <<(bytes)
        appended = bytes.to_s.b
        if @bytes.bytesize + appended.bytesize > @limit
          raise OutputLimitExceeded,
                "output limit exceeded: #{@bytes.bytesize + appended.bytesize} > #{@limit}"
        end

        @bytes << appended
        self
      end

      # @return [Integer] number of bytes currently stored.
      def bytesize
        @bytes.bytesize
      end

      # @return [Boolean] whether the buffer is empty.
      def empty?
        @bytes.empty?
      end

      # @return [String] accumulated bytes as a UTF-8 String. Bytes that are
      #   not valid UTF-8 are returned with the original BINARY encoding —
      #   the host is responsible for downstream re-encoding decisions.
      def to_s
        copy = @bytes.dup
        copy.force_encoding(Encoding::UTF_8)
        copy.valid_encoding? ? copy : @bytes.dup
      end

      # Reset the buffer to empty. Used at the per-`#run` boundary (item #16).
      def clear
        @bytes.clear
        self
      end
    end

    attr_reader :wasm_path, :engine, :module_, :store, :instance,
                :handle_table, :stdout_buffer, :stderr_buffer,
                :stdout_limit, :stderr_limit, :services

    # Build a fresh Sandbox.
    #
    # @param wasm_path [String, nil] absolute path to the Guest Binary
    #   (`kobako.wasm`). Defaults to `Kobako::Wasm.default_path`. If the
    #   file does not exist, raises `Kobako::Wasm::ModuleNotBuiltError`.
    # @param stdout_limit [Integer, nil] per-run stdout byte ceiling.
    #   Defaults to 1 MiB.
    # @param stderr_limit [Integer, nil] per-run stderr byte ceiling.
    #   Defaults to 1 MiB.
    # @param engine [Kobako::Wasm::Engine, nil] optional shared Engine. When
    #   nil, a fresh Engine is constructed for this Sandbox (fine for tests,
    #   wasteful in production loops — share at the call site).
    def initialize(wasm_path: nil, stdout_limit: nil, stderr_limit: nil, engine: nil)
      @wasm_path = wasm_path || Kobako::Wasm.default_path
      @stdout_limit = stdout_limit || DEFAULT_OUTPUT_LIMIT
      @stderr_limit = stderr_limit || DEFAULT_OUTPUT_LIMIT
      build_wasm_pipeline(engine)
      @handle_table = Kobako::HandleTable.new
      @stdout_buffer = OutputBuffer.new(@stdout_limit)
      @stderr_buffer = OutputBuffer.new(@stderr_limit)
      @services = Kobako::Service::Registry.new
    end

    # Declare or retrieve a Service Group on this Sandbox (SPEC §B-07,
    # B-09, B-10). Idempotent — repeat calls with the same name return
    # the same Group instance.
    #
    # @param name [Symbol, String] constant-form group name.
    # @return [Kobako::Service::Group]
    # @raise [ArgumentError] when called after `#run`, or when the name
    #   does not match the constant-name pattern.
    def define(name)
      @services.define(name)
    end

    # Execute a guest mruby script (SPEC §B-02 / §B-03).
    #
    # Drives the four-step ABI flow against the cached Wasm instance:
    #
    #   1. Reset per-run state (HandleTable, capture buffers); seal the
    #      Service Registry on first call (B-15, B-19, B-07 Notes).
    #   2. Allocate guest linear memory for the source bytes via
    #      `__kobako_alloc(len)`, then write the source bytes there.
    #   3. Invoke `__kobako_run` (passing the source ptr/len when the
    #      guest accepts that shape; the production Guest Binary takes
    #      no args and reads source via WASI stdin — that path lands in
    #      a later item).
    #   4. Read OUTCOME_BUFFER via `__kobako_take_outcome` and decode the
    #      envelope. Result envelope returns the wrapped value; Panic
    #      envelope raises {Kobako::SandboxError} or
    #      {Kobako::ServiceError} based on `origin`.
    #
    # @param source [String] mruby source code (UTF-8).
    # @return [Object] the deserialized last expression of the script.
    # @raise [Kobako::TrapError]    Wasm trap or wire-violation fallback.
    # @raise [Kobako::SandboxError] guest ran to completion but the
    #   execution failed (mruby error, decode failure, panic with
    #   `origin: "sandbox"`).
    # @raise [Kobako::ServiceError] guest ran a Service call that raised
    #   and the script did not rescue (`origin: "service"`).
    def run(source)
      raise SandboxError, "source must be a String, got #{source.class}" unless source.is_a?(String)

      @services.seal!
      reset_run_state!

      ptr, len = inject_source(source)
      invoke_guest_run(ptr, len)
      outcome_bytes = read_outcome_bytes
      decode_outcome_value(outcome_bytes)
    end

    private

    # Per-run state reset (SPEC §B-03). Capture buffers and the
    # HandleTable counter are zeroed before the guest runs.
    def reset_run_state!
      @handle_table.reset!
      @stdout_buffer.clear
      @stderr_buffer.clear
    end

    # Allocate a buffer in the guest and copy +source+ bytes into it.
    # Returns the [ptr, len] pair so the Run import receives them. A
    # ptr of 0 from the guest indicates an allocation trap.
    def inject_source(source)
      bytes = source.b
      len = bytes.bytesize
      ptr = @instance.alloc(len)
      raise TrapError, "guest __kobako_alloc returned 0 (allocation failure)" if ptr.zero?

      @instance.write_memory(ptr, bytes)
      [ptr, len]
    rescue Kobako::Wasm::Error => e
      raise TrapError, "failed to inject source into guest: #{e.message}"
    end

    # Invoke `__kobako_run`. Wraps wasmtime / wire errors in TrapError
    # so the Host App can recreate the Sandbox per SPEC §B-02 attribution
    # rules.
    def invoke_guest_run(ptr, len)
      @instance.run(ptr, len)
    rescue Kobako::Wasm::Error => e
      raise TrapError, "guest __kobako_run trapped: #{e.message}"
    end

    # Pull the OUTCOME_BUFFER bytes out of guest memory. SPEC pins
    # `len == 0` as a wire violation that surfaces as TrapError.
    def read_outcome_bytes
      packed = @instance.take_outcome
      ptr, len = unpack_packed_u64(packed)
      raise TrapError, "guest exited without writing an outcome (len=0)" if len.zero?

      @instance.read_memory(ptr, len)
    rescue Kobako::Wasm::Error => e
      raise TrapError, "failed to read OUTCOME_BUFFER: #{e.message}"
    end

    # Big-endian unpack of the SPEC packed u64 layout: high 32 bits =
    # ptr, low 32 bits = len.
    def unpack_packed_u64(packed)
      ptr = (packed >> 32) & 0xffff_ffff
      len = packed & 0xffff_ffff
      [ptr, len]
    end

    # Decode the OUTCOME envelope and dispatch on the tag (SPEC §"Step 2
    # — Outcome envelope tag"). Result → return value; Panic → raise
    # SandboxError or ServiceError; anything else → TrapError.
    def decode_outcome_value(bytes)
      outcome = Kobako::Wire::Envelope.decode_outcome(bytes)
      case outcome.payload
      when Kobako::Wire::Envelope::Result
        outcome.payload.value
      when Kobako::Wire::Envelope::Panic
        raise build_panic_error(outcome.payload)
      end
    rescue Kobako::Wire::Error => e
      # Malformed envelope: per SPEC, an unknown outcome tag or a
      # truncated payload is a wire-violation fallback to TrapError.
      raise TrapError, "outcome envelope decode failed: #{e.message}"
    end

    # Map a decoded {Kobako::Wire::Envelope::Panic} into the
    # corresponding three-layer Ruby exception. `origin == "service"`
    # → ServiceError; everything else → SandboxError (SPEC §"Step 2"
    # attribution table).
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

    def build_wasm_pipeline(engine)
      @engine = engine || Kobako::Wasm::Engine.new
      @module_ = Kobako::Wasm::Module.from_file(@engine, @wasm_path)
      @store = Kobako::Wasm::Store.new(@engine)
      @instance = Kobako::Wasm::Instance.new(@engine, @module_, @store)
    end
  end
end
