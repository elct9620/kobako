# frozen_string_literal: true

require "test_helper"

# Sandbox#flag through the bundled Guest Binary. That binary recognises no
# flag, so every flag set here meets the unrecognised path: a critical one
# fails the invocation before any script runs, and an advisory one is
# skipped. Name and value validation is pinned at the table tier in
# test/unit/catalog/test_flags.rb.
class TestSandboxFlag < Minitest::Test
  include E2eGuestHelper

  def setup
    super
    @sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
  end

  # @behavior S-180
  def test_flag_returns_self_for_chaining
    assert_same @sandbox, @sandbox.flag(:strict),
                "a flag through Sandbox#flag must answer the Sandbox for chaining"
  end

  # @behavior S-181
  def test_flag_rejects_calls_after_first_invocation
    @sandbox.eval("nil")

    err = assert_raises(ArgumentError) { @sandbox.flag(:late) }
    assert_match(/after first Sandbox invocation/, err.message,
                 "a flag through Sandbox#flag after the first invocation must be refused naming it")
  end

  # @behavior S-182
  def test_flag_rejects_a_name_set_twice
    @sandbox.flag(:strict)

    assert_raises(ArgumentError, "a flag name set twice through Sandbox#flag must be refused") do
      @sandbox.flag("strict", "on")
    end
  end

  # The script prints before it answers, so empty stdout witnesses that it
  # never ran.
  # @behavior S-183
  def test_a_critical_flag_the_guest_does_not_recognise_fails_the_invocation
    @sandbox.flag(:no_such_flag)

    err = assert_raises(Kobako::UnsupportedFlagError) { @sandbox.eval("puts 'ran'; 1") }
    assert_equal ["", true], [err.execution.stdout.to_s, err.message.include?("no_such_flag")],
                 "a critical flag the Guest Binary does not recognise through Sandbox#eval must " \
                 "raise UnsupportedFlagError naming it before the script runs"
  end

  # @behavior S-184
  def test_an_advisory_flag_the_guest_does_not_recognise_is_skipped
    @sandbox.flag(:no_such_flag, "8", advisory: true)

    assert_equal 2, @sandbox.eval("1 + 1").value,
                 "an advisory flag the Guest Binary does not recognise through Sandbox#eval must " \
                 "be skipped, leaving the value as it would be without the flag"
  end
end
