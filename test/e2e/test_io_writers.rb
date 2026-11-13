# frozen_string_literal: true

require "test_helper"

# E2E — what the Kernel writers write to and how they take their arguments,
# through real mruby, held to CRuby's: a rebound channel reached through its
# write, printf and warn's argument forms, an IO with no p, and puts of an
# Array holding itself.
class TestE2EIoWriters < Minitest::Test
  include E2eGuestHelper

  # A channel answering only write sees every Kernel writer but puts, which
  # CRuby hands to the rebound object's own puts.
  REBOUND_SCRIPT = <<~RUBY
    out = Class.new { attr_reader :got; def initialize; @got = []; end; def write(*s); @got << s.join; end }
    $stdout = o = out.new
    $stderr = e = out.new
    print "a"; printf("%d", 1); p :b; warn "c"
    $stdout = STDOUT; $stderr = STDERR
    [o.got.join, e.got.join]
  RUBY

  # @behavior IO-049
  def test_the_kernel_writers_reach_a_rebound_channel_through_its_write
    execution = Kobako::Sandbox.new(wasm_path: REAL_WASM).eval(REBOUND_SCRIPT)

    assert_equal ["a1:b\n", "c\n"], execution.value,
                 "print, printf, p, and warn through Sandbox#eval must reach a rebound $stdout or $stderr " \
                 "answering only write"
  end

  # @behavior IO-050
  def test_printf_and_warn_take_their_arguments_as_crubys_do
    source = 'printf; warn; printf($stderr, "%s", "e"); warn("w", uplevel: 0); 1'
    execution = Kobako::Sandbox.new(wasm_path: REAL_WASM).eval(source)

    assert_equal ["", "ew\n"], [execution.stdout, execution.stderr],
                 "bare printf and warn through Sandbox#eval must write nothing, printf with $stderr first " \
                 "must write there, and warn with uplevel: must write its message alone"
  end

  # @behavior IO-051
  def test_an_io_answers_no_p
    source = "begin; $stdout.p(1); rescue NoMethodError => e; e.class.to_s; end"
    execution = Kobako::Sandbox.new(wasm_path: REAL_WASM).eval(source)

    assert_equal ["NoMethodError", ""], [execution.value, execution.stdout],
                 "$stdout.p through Sandbox#eval must raise NoMethodError, as CRuby's IO defines no p"
  end

  # @behavior IO-053
  def test_an_array_holding_itself_is_put_as_a_placeholder
    execution = Kobako::Sandbox.new(wasm_path: REAL_WASM).eval("a = [1]; a << a; puts a; 1")

    assert_equal "1\n[...]\n", execution.stdout,
                 "Kernel#puts of an Array holding itself through Sandbox#eval must write [...] for the inner one"
  end
end
