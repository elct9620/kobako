# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the kobako-io Kernel delegators through real mruby
# (SPEC.md B-04): putc byte semantics, private registration, p's inspect
# form, and puts' Array flattening / GC-arena behaviour. Channel routing
# lives in test_io_streams.rb; IO write byte paths in test_io_write.rb.
class TestE2EIoKernel < Minitest::Test
  include E2eGuestHelper

  # SPEC.md B-04: Kernel#putc routes through $stdout, Integer arg writes a
  # single byte (c & 0xff). Pins alignment with mruby-io's mrblib/kernel.rb
  # putc surface (vendor/mruby/mrbgems/mruby-io/mrblib/kernel.rb:95-98).
  def test_putc_integer_writes_byte_to_stdout
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval("putc 65; 1")

    assert_equal "A", sandbox.stdout,
                 "Kernel#putc with Integer must write the byte (c & 0xff) to $stdout"
    assert_empty sandbox.stderr,
                 "Kernel#putc must not bleed into stderr"
  end

  # SPEC.md B-04: Kernel#putc with an Integer masks with +& 0xff+ before
  # writing — mirrors mruby-io's +io_putc+ in
  # vendor/mruby/mrbgems/mruby-io/src/io.c:1103. The companion test
  # +test_putc_integer_writes_byte_to_stdout+ uses +putc 65+ where the
  # mask is the identity; this one feeds +putc 321+ (321 & 0xff == 65)
  # so dropping the mask would silently write +"Ł"+-ish bytes
  # instead of +"A"+ and the assertion would catch the drift.
  def test_putc_integer_masks_byte
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval("putc 321; 1")

    assert_equal "A", sandbox.stdout,
                 "Kernel#putc with Integer must mask via (c & 0xff); 321 → 65 → 'A'"
    assert_empty sandbox.stderr,
                 "Kernel#putc must not bleed into stderr"
  end

  # SPEC.md B-04: Kernel#putc returns +nil+, not the argument — pinned
  # by mruby-io's mrblib/kernel.rb:95-98. The IO-level +IO#putc+
  # returns the original object; the Kernel delegator deliberately
  # drops it. If anyone collapses the Kernel#putc body back to a
  # one-liner delegate, IO#putc's +obj+ would bleed through and this
  # assertion catches the drift.
  def test_kernel_putc_returns_nil
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    result = sandbox.eval("putc 65")

    assert_nil result,
               "Kernel#putc must return nil (mruby-io alignment), not the obj that IO#putc returns"
    assert_equal "A", sandbox.stdout,
                 "putc 65 must still land on stdout"
  end

  # SPEC.md B-04: the Kernel delegators register private, matching the
  # mruby-io mrblib declaration (+module Kernel; private; def puts ...+).
  # mruby 4 enforces visibility at VM dispatch, so a public registration
  # would be observably different: +42.puts("x")+ would write to the
  # capture pipe instead of raising. Unrescued, the raise reaches the
  # host as SandboxError (E-04) carrying the guest exception class.
  def test_kernel_delegators_register_private
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) { sandbox.eval('42.puts("x")') }

    assert_equal "NoMethodError", err.klass,
                 "explicit-receiver dispatch of a Kernel delegator through the guest must " \
                 "raise NoMethodError (private visibility), not write to the capture pipe"
    assert_empty sandbox.stdout,
                 "a private Kernel#puts must not leak output through an explicit receiver"
  end

  # SPEC.md B-04: Kernel#putc with a String writes the first character.
  # Mruby is compiled without MRB_UTF8_STRING, so the first character is
  # the first byte — same behavior as mruby-io's non-UTF8 fallback path
  # (vendor/mruby/mrbgems/mruby-io/src/io.c:1125-1129).
  def test_putc_string_writes_first_character_to_stdout
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval('putc "Zed"; 1')

    assert_equal "Z", sandbox.stdout,
                 "Kernel#putc with String must write only the first character to $stdout"
    assert_empty sandbox.stderr,
                 "Kernel#putc must not bleed into stderr"
  end

  # SPEC.md B-04: Kernel#p writes inspect form to $stdout (not the raw to_s).
  # Pins the inspect-format invariant that distinguishes #p from #puts.
  def test_p_writes_inspect_form_to_stdout
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval("p({a: 1}); 1")

    assert_includes sandbox.stdout, "{a: 1}",
                    "Kernel#p must write Hash inspect form to stdout (mruby 4.0 shorthand)"
  end

  # SPEC.md B-04: the IO write loops run in C frames, where mruby's GC
  # arena (100 slots) is not restored per instruction the way it is
  # under the VM; each iteration allocates at least a coerced String
  # and a newline, so 150 arguments overflow the arena unless the loop
  # brackets every iteration in an arena scope. Witness: dropping the
  # scope makes mruby raise its arena-overflow error mid-loop, which
  # surfaces as SandboxError instead of the full output.
  def test_puts_long_argument_list_does_not_overflow_gc_arena
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval("puts(*(1..150).to_a); 1")

    assert_equal (1..150).map { |i| "#{i}\n" }.join, sandbox.stdout,
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

  # SPEC.md B-04: Kernel#puts flattens Array arguments element-wise, and
  # the recursion gate is is_a?(Array) — an Array *subclass* instance
  # must flatten too, not stringify wholesale through to_s.
  def test_puts_flattens_array_subclass_elementwise
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval(ARRAY_SUBCLASS_PUTS_SCRIPT)

    assert_equal "first\nsecond\n", sandbox.stdout,
                 "Kernel#puts must flatten an Array subclass element-wise, " \
                 "matching the is_a?(Array) recursion gate"
  end
end
