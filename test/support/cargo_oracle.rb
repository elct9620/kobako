# frozen_string_literal: true

require "open3"

# Test helper for driving a Rust "oracle" subprocess from a Minitest test.
#
# The oracle pattern (used by SPEC items #7 / #8):
#
#   1. Build a Rust binary in +wasm/kobako-wasm/target/release/+ via
#      +cargo build --release --manifest-path ...+. The build result is
#      memoised per binary name so a slow cargo invocation happens once
#      even when multiple tests share the helper.
#   2. Spawn the binary as a long-lived subprocess via +Open3.popen2+;
#      both pipes are switched to +binmode+ so binary payloads survive
#      intact.
#   3. Exchange length-prefixed binary frames over stdin / stdout. The
#      frame header is a 4-byte big-endian +u32+ whose high bit is an
#      error flag and whose low 31 bits encode the payload length.
#
# +cargo+ is not assumed to be on PATH. Callers check +#ensure_built.status+
# and +skip+ on +:no_cargo+ (clean checkout without Rust toolchain) or
# +flunk+ on +:build_failed+ (build broken — surface the captured output).
class CargoOracle
  ERROR_FLAG = 0x8000_0000

  # Outcome of a one-shot +cargo build --release+ for a single binary.
  # +status+ is one of +:ok+, +:no_cargo+, +:build_failed+; +error+
  # carries the captured combined stdout/stderr on +:build_failed+ and
  # is +nil+ otherwise.
  BuildResult = Data.define(:status, :error)

  # Per-test-process build cache, keyed by +bin_name+. Indexed by name
  # rather than by +CargoOracle+ instance so two tests asking for the
  # same oracle (constructed independently) still share the build.
  @builds = {}

  class << self
    attr_reader :builds
  end

  # +crate_dir+ is the Rust crate root (the directory containing
  # +Cargo.toml+). +bin_name+ is the +[[bin]]+ target the oracle is
  # built from.
  def initialize(crate_dir:, bin_name:)
    @crate_dir = crate_dir
    @bin_name = bin_name
    @binary_path = File.join(crate_dir, "target", "release", bin_name)
  end

  attr_reader :binary_path, :bin_name

  # Memoised build of the oracle binary. Returns a +BuildResult+. Safe
  # to call from +setup+ on every test method — the actual cargo
  # invocation happens once per process.
  def ensure_built
    self.class.builds[@bin_name] ||= cargo_build
  end

  # Spawn the oracle subprocess and return a +Process+ facade. The
  # caller owns the lifecycle — call +#close+ in +teardown+ or wrap
  # the whole exchange in +#open+ to get automatic cleanup.
  def spawn
    Process.new(*Open3.popen2(@binary_path))
  end

  # Spawn the oracle, yield the +Process+ facade, and close it on block
  # exit (including on exception). Preferred for tests that exchange
  # many frames inside a single test method.
  def open
    process = spawn
    yield process
  ensure
    process&.close
  end

  private

  def cargo_build
    return BuildResult.new(status: :no_cargo, error: nil) unless cargo_on_path?

    out, status = Open3.capture2e(
      "cargo", "build", "--release",
      "--manifest-path", File.join(@crate_dir, "Cargo.toml"),
      "--bin", @bin_name
    )
    return BuildResult.new(status: :ok, error: nil) if status.success? && File.executable?(@binary_path)

    BuildResult.new(status: :build_failed, error: out)
  end

  def cargo_on_path?
    system("command -v cargo > /dev/null 2>&1")
  end

  # Wraps the subprocess I/O. +send_frame+ writes a length-prefixed
  # frame; +read_frame+ returns +[body, error_flag]+ where +body+ is
  # the binary payload and +error_flag+ is +true+ when the oracle set
  # the high bit of the length word. Both pipes are forced to
  # +binmode+ at construction so callers never need to remember.
  class Process
    def initialize(stdin, stdout, wait_thr)
      @stdin = stdin
      @stdout = stdout
      @wait_thr = wait_thr
      @stdin.binmode
      @stdout.binmode
    end

    def send_frame(payload)
      @stdin.write([payload.bytesize].pack("N"))
      @stdin.write(payload)
      @stdin.flush
    end

    # Read one response frame. Returns +[body, error_flag]+. Raises
    # +EOFError+ if the stream closed before a full header arrived, or
    # +IOError+ if the body is shorter than the header advertised.
    def read_frame
      word = read_header
      body = read_body(word & ~ERROR_FLAG)
      [body.b, word.anybits?(ERROR_FLAG)]
    end

    def close
      safe_close(@stdin)
      drain_stdout
      safe_close(@stdout)
      @wait_thr&.join
    end

    private

    def read_header
      hdr = @stdout.read(4)
      raise EOFError, "oracle stdout closed; no header" if hdr.nil? || hdr.bytesize < 4

      hdr.unpack1("N")
    end

    def read_body(len)
      return "".b if len.zero?

      body = @stdout.read(len)
      return body unless body.nil? || body.bytesize != len

      raise IOError, "oracle stdout truncated (header said #{len} bytes, got #{body&.bytesize.inspect})"
    end

    def safe_close(io)
      io&.close unless io&.closed?
    end

    # Drain any remaining stdout so the child can exit cleanly. The
    # rescue is intentional — we only care about a tidy shutdown, not
    # the bytes themselves.
    def drain_stdout
      @stdout&.read
    rescue StandardError
      nil
    end
  end
end
