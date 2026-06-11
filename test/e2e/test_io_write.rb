# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the kobako-io ::IO class surface through real mruby
# (SPEC.md B-04): construction validation, fileno, the mruby-io-compatible
# supplementary surface, IO#write byte-pumping fidelity, and the
# mruby-sprintf formatting capability. Kernel delegators live in
# test_io_kernel.rb; channel routing in test_io_streams.rb.
class TestE2EIoWrite < Minitest::Test
  include E2eGuestHelper

  # Guest IO is scoped to the two captured descriptors; any other fd
  # raises ArgumentError at construction so the failure is loud rather
  # than a silent fwrite to a no-op stream.
  def test_io_new_rejects_unsupported_fd
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval('IO.new(99, "w")')
    end

    assert_includes err.message, "kobako IO only supports fd",
                    "io_initialize must raise ArgumentError citing the fd constraint"
  end

  # Mirror of fd validation for the mode argument — only "w" is
  # supported because mruby-io's read-path is intentionally out of
  # scope (see the kobako-io IO surface, wasm/kobako-io/src/io.rs).
  def test_io_new_rejects_unsupported_mode
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval('IO.new(1, "r")')
    end

    assert_includes err.message, 'kobako IO only supports mode "w"',
                    "io_initialize must raise ArgumentError citing the mode constraint"
  end

  # Pins the io_fileno C bridge through a real run: STDOUT was
  # constructed with fd 1 in install_raw, so STDOUT.fileno must round
  # trip back to 1. STDERR.fileno mirrors with 2.
  def test_stdout_and_stderr_fileno_return_underlying_descriptor
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_equal 1, sandbox.eval("STDOUT.fileno")
    assert_equal 2, sandbox.eval("STDERR.fileno")
  end

  # Probe script for the supplementary IO surface — each member's
  # contract value collected into one Array for the outcome path.
  SUPPLEMENTARY_IO_SCRIPT = <<~RUBY
    chained = ($stdout << "a" << "b").equal?($stdout)
    [chained, $stdout.tty?, $stdout.sync, ($stdout.sync = false),
     $stdout.sync, $stdout.flush.equal?($stdout), $stdout.closed?, $stdout.to_i]
  RUBY

  # SPEC.md B-04: the mruby-io-compatible supplementary IO surface —
  # `<<` chaining, tty? / sync / sync= / flush / closed? introspection,
  # and the to_i alias — stays drop-in compatible so scripts written
  # against mruby-io run unchanged. `<<` additionally lands its bytes
  # on the stdout capture channel.
  def test_io_supplementary_surface_matches_mruby_io
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval(SUPPLEMENTARY_IO_SCRIPT)

    assert_equal [true, false, true, false, false, true, false, 1], result,
                 "IO supplementary surface (<< self-chain, tty?, sync default/assignment, " \
                 "flush self-return, closed?, to_i alias) must match the mruby-io contract"
    assert_equal "ab", sandbox.stdout,
                 "$stdout << must write its argument bytes to the stdout capture channel"
  end

  # IO#write byte-pumping coverage — pins the two paths the safe
  # layer exercises through every `print` / `puts` / `$stdout.write`:
  # `mrb_obj_as_string` coercion (already-String vs to_s detour) and
  # the `mrb_rstring_ptr` / `mrb_rstring_len` static-inline wrappers
  # that follow the embed-vs-heap RString branch. A drift in the
  # `wrapper.h` macro expansion or the `RString` header layout would
  # surface as a mismatched byte assertion below.

  # Strings short enough to fit inside RStringEmbed.ary go through
  # the embed branch of RSTRING_PTR / RSTRING_LEN. 11 bytes sits at
  # the inline boundary (`RSTRING_EMBED_LEN_MAX` on wasm32) — a
  # regression that read past the embed buffer or returned the
  # wrong length would corrupt the captured output.
  def test_io_write_round_trips_embed_tagged_string
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval('print "abcdefghijk"')
    assert_equal "abcdefghijk", sandbox.stdout,
                 "short string passed to `print` must reach stdout intact"
  end

  # Strings beyond the embed cap live in as_.heap.{ptr,len}; the
  # same wrappers must follow the heap-pointer branch. 100 bytes
  # is well clear of the boundary so any embed-only regression
  # would yield a truncated or zero-length capture. mruby builds
  # the string itself via `"x" * 100` so the test does not need
  # Ruby-side interpolation.
  def test_io_write_round_trips_heap_tagged_string
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval('print "x" * 100')
    assert_equal "x" * 100, sandbox.stdout,
                 "long string passed to `print` must reach stdout intact"
  end

  # IO#write routes through `write(2)` with an explicit `ptr + len`,
  # not `mrb_str_to_cstr` (which would truncate at the first NUL
  # byte). Embedded NUL must reach the capture pipe intact.
  def test_io_write_preserves_embedded_nul_bytes
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval("print \"a\\0b\"")
    assert_equal "a\0b".b, sandbox.stdout.b,
                 "NUL bytes inside a `print` payload must reach stdout"
  end

  # `mrb_obj_as_string` on a value that is already a String returns
  # the receiver unchanged — no Object#to_s detour. The literal's
  # bytes reach `write(2)` verbatim.
  def test_io_write_passes_through_already_string_without_coercion
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval('print "literal-string"')
    assert_equal "literal-string", sandbox.stdout,
                 "String argument to `print` must reach stdout verbatim"
  end

  # `mrb_obj_as_string` on a non-String calls Object#to_s. Integer
  # 42 round-trips as the canonical "42" decimal string; a skipped
  # coercion path would surface a raw boxed representation (or
  # trap).
  def test_io_write_coerces_non_string_via_to_s
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval("print 42")
    assert_equal "42", sandbox.stdout,
                 "Integer argument to `print` must reach stdout as its `to_s` form"
  end

  # ── sprintf / String#% / IO#printf — the mruby-sprintf mrbgem ────────
  #
  # mruby-sprintf supplies Kernel#sprintf, String#% and the format engine
  # they share; without it in the allowlist these methods are absent and
  # kobako-io's `IO#printf` (which calls `sprintf`) raises NoMethodError.
  # These journeys exercise the capability through the public `#eval` API:
  # the outcome path proves sprintf/% return formatted Strings, and the
  # stdout path proves `printf` writes the formatted bytes to the capture
  # channel.

  # Kernel#sprintf must apply width / precision specifiers and return the
  # formatted String through the outcome envelope.
  def test_sprintf_formats_value_through_eval
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval('sprintf("%05.2f", 3.14159)')

    assert_equal "03.14", result,
                 "sprintf through #eval must apply width/precision and return the formatted String"
  end

  # String#% must route through the same format engine, threading an Array
  # of arguments into positional specifiers.
  def test_string_percent_formats_array_through_eval
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval('"%d-%s" % [42, "x"]')

    assert_equal "42-x", result,
                 "String#% through #eval must interpolate the Array into positional specifiers"
  end

  # kobako-io's IO#printf delegates to sprintf, so a guest `printf`
  # call must write the formatted bytes to the stdout capture channel —
  # the latent NoMethodError this gem fixes surfaced exactly here.
  def test_printf_writes_formatted_output_to_stdout
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    sandbox.eval('printf("%03d\n", 7)')

    assert_equal "007\n", sandbox.stdout,
                 "printf through #eval must write the sprintf-formatted bytes to Sandbox#stdout"
  end
end
