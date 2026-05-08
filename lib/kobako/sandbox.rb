# frozen_string_literal: true

require_relative "handle_table"
require_relative "service"

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
    # B-09, B-10; REFERENCE Ch.6 §Service 注入 API). Idempotent — repeat
    # calls with the same name return the same Group instance.
    #
    # @param name [Symbol, String] constant-form group name.
    # @return [Kobako::Service::Group]
    # @raise [ArgumentError] when called after `#run`, or when the name
    #   does not match the constant-name pattern.
    def define(name)
      @services.define(name)
    end

    # Execute a guest mruby script. Implemented in item #16; not yet wired.
    def run(_script_string)
      @services.seal!
      raise NotImplementedError,
            "Kobako::Sandbox#run is not yet implemented (implemented in item #16)"
    end

    private

    def build_wasm_pipeline(engine)
      @engine = engine || Kobako::Wasm::Engine.new
      @module_ = Kobako::Wasm::Module.from_file(@engine, @wasm_path)
      @store = Kobako::Wasm::Store.new(@engine)
      @instance = Kobako::Wasm::Instance.new(@engine, @module_, @store)
    end
  end
end
