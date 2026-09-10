# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the kobako-io Kernel delegators through real mruby:
# putc byte semantics, private registration, p's inspect form, and puts'
# Array flattening / GC-arena behaviour. Channel routing lives in
# test_io_streams.rb; IO write byte paths in test_io_write.rb.
class TestE2EIoKernel < Minitest::Test
  include E2eGuestHelper

  # @behavior IO-023 IO-024
  # Pins alignment with mruby-io's putc surface
  # (vendor/mruby/mrbgems/mruby-io/mrblib/kernel.rb:95-98).
  def test_putc_integer_writes_byte_to_stdout
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    execution = sandbox.eval("putc 65; 1")

    assert_equal "A", execution.stdout,
                 "Kernel#putc with Integer must write the byte (c & 0xff) to $stdout"
    assert_empty execution.stderr,
                 "Kernel#putc must not bleed into stderr"
  end

  # @behavior IO-024 IO-025
  # Mirrors mruby-io's +io_putc+ mask (vendor/mruby/mrbgems/mruby-io/src/io.c:1103).
  # +putc 321+ (321 & 0xff == 65) is an input where the mask is not the
  # identity, so dropping it would write a byte other than +"A"+.
  def test_putc_integer_masks_byte
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    execution = sandbox.eval("putc 321; 1")

    assert_equal "A", execution.stdout,
                 "Kernel#putc with Integer must mask via (c & 0xff); 321 → 65 → 'A'"
    assert_empty execution.stderr,
                 "Kernel#putc must not bleed into stderr"
  end

  # @behavior IO-023 IO-026
  # Pinned by mruby-io's mrblib/kernel.rb:95-98: +IO#putc+ returns its
  # argument, but the Kernel delegator deliberately drops it, so collapsing
  # the delegator into a one-liner would leak IO#putc's +obj+.
  def test_kernel_putc_returns_nil
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    execution = sandbox.eval("putc 65")

    assert_nil execution.value,
               "Kernel#putc must return nil (mruby-io alignment), not the obj that IO#putc returns"
    assert_equal "A", execution.stdout,
                 "putc 65 must still land on stdout"
  end

  # @behavior IO-027 IO-028
  # The delegators match mruby-io's private mrblib declaration; mruby 4
  # enforces visibility at VM dispatch, so a public registration would let
  # +42.puts("x")+ write to the capture pipe instead of raising.
  def test_kernel_delegators_register_private
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) { sandbox.eval('42.puts("x")') }

    assert_equal "NoMethodError", err.klass,
                 "explicit-receiver dispatch of a Kernel delegator through the guest must " \
                 "raise NoMethodError (private visibility), not write to the capture pipe"
    assert_empty err.execution.stdout,
                 "a private Kernel#puts must not leak output through an explicit receiver"
  end

  # @behavior IO-024 IO-029
  # Mruby is compiled without MRB_UTF8_STRING, so the first character is
  # the first byte — same behavior as mruby-io's non-UTF8 fallback path
  # (vendor/mruby/mrbgems/mruby-io/src/io.c:1125-1129).
  def test_putc_string_writes_first_character_to_stdout
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    execution = sandbox.eval('putc "Zed"; 1')

    assert_equal "Z", execution.stdout,
                 "Kernel#putc with String must write only the first character to $stdout"
    assert_empty execution.stderr,
                 "Kernel#putc must not bleed into stderr"
  end

  # @behavior IO-030
  # Pins the inspect-format invariant that distinguishes #p from #puts.
  def test_p_writes_inspect_form_to_stdout
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    execution = sandbox.eval("p({a: 1}); 1")

    assert_includes execution.stdout, "{a: 1}",
                    "Kernel#p must write Hash inspect form to stdout (mruby 4.0 shorthand)"
  end

  # @behavior IO-031
  # The IO write loops run in C frames, where mruby's 100-slot GC arena is
  # not restored per instruction, so 150 arguments overflow it unless every
  # iteration is bracketed in an arena scope; dropping the scope surfaces as
  # SandboxError instead of the full output.
  def test_puts_long_argument_list_does_not_overflow_gc_arena
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    execution = sandbox.eval("puts(*(1..150).to_a); 1")

    assert_equal (1..150).map { |i| "#{i}\n" }.join, execution.stdout,
                 "Kernel#puts with 150 arguments must write every line; a long argument " \
                 "list must not abort the guest mid-loop"
  end

  # Builds an Array *subclass* instance and puts it — the flattening
  # witness for the is_a?(Array) recursion gate.
  ARRAY_SUBCLASS_PUTS_SCRIPT = <<~RUBY
    class Lines < Array; end
    list = Lines.new
    list << "first" << "second"
    puts list
    1
  RUBY

  # @behavior IO-032
  # The recursion gate is is_a?(Array), so an Array *subclass* instance
  # must flatten too rather than stringify wholesale through to_s.
  def test_puts_flattens_array_subclass_elementwise
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    execution = sandbox.eval(ARRAY_SUBCLASS_PUTS_SCRIPT)

    assert_equal "first\nsecond\n", execution.stdout,
                 "Kernel#puts must flatten an Array subclass element-wise, " \
                 "matching the is_a?(Array) recursion gate"
  end
end
