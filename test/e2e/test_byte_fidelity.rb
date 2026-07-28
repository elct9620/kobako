# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — what a guest String or Symbol's bytes do on the way out,
# across every guest→host value path: the outcome (#eval return), a
# dispatch argument, and a dispatch keyword name.
#
# docs/wire/payload-msgpack.md § Text and Bytes: an mruby String is a byte
# array with no encoding tag, so the guest encoder has exactly one rule
# available — bytes that are valid UTF-8 ride as wire str, anything else
# as bin. A Symbol has no such second family (its name rides as ext 0x00,
# whose payload the wire requires to be UTF-8), so a name that is not
# UTF-8 has no representation and is refused: E-06 on the outcome path,
# E-55 on a dispatch argument or keyword name.
#
# These are witnesses rather than fuzz cases because the round-trip fuzz
# in test/fuzz/test_guest_value_fuzz.rb generates only wire-legal values:
# it reaches the String rule from both sides, but never a name the wire
# cannot carry. The regression they guard against is silent — reading a
# String through a `to_s` render answers the empty string for every byte
# sequence a Rust `String` cannot hold, so the value disappears with no
# raise and no truncation for a caller to notice.
class TestE2EByteFidelity < Minitest::Test
  include E2eGuestHelper

  # Bytes chosen so the string is ASCII everywhere except one 0xFF, which
  # no UTF-8 sequence can begin: valid UTF-8 up to that byte, invalid at it.
  # The guest spelling is the same escape, which mruby reads as one byte.
  NON_UTF8 = "bin\xFFkey"
  NON_UTF8_SOURCE = '"bin\xFFkey"'

  def test_outcome_non_utf8_string_keeps_its_bytes_as_binary
    result = Kobako::Sandbox.new(wasm_path: REAL_WASM).eval(NON_UTF8_SOURCE).value

    assert_equal NON_UTF8.b, result,
                 "a String whose bytes are not UTF-8 through #eval must arrive with those bytes intact"
    assert_equal Encoding::ASCII_8BIT, result.encoding,
                 "a String that rode as wire bin through #eval must arrive ASCII-8BIT-tagged"
  end

  def test_outcome_non_utf8_symbol_is_refused
    assert_raises(Kobako::SandboxError,
                  "E-06: a Symbol whose name is not UTF-8 through #eval must be refused, " \
                  "not interned under a name the guest never wrote") do
      Kobako::Sandbox.new(wasm_path: REAL_WASM).eval('"s\xFFy".to_sym')
    end
  end

  def test_dispatch_argument_non_utf8_string_reaches_the_service_intact
    seen = nil
    sandbox = probe_sandbox { |value| seen = value }

    sandbox.eval('Probe::Sink.call("bin\xFFkey")')

    assert_equal NON_UTF8.b, seen,
                 "a String whose bytes are not UTF-8 through a dispatch argument must reach " \
                 "the Service with those bytes intact"
  end

  def test_dispatch_argument_non_utf8_symbol_is_refused
    sandbox = probe_sandbox

    assert_raises(Kobako::SandboxError,
                  "E-55: a Symbol whose name is not UTF-8 through a dispatch argument must be " \
                  "refused at the guest call site, not renamed") do
      sandbox.eval('Probe::Sink.call("s\xFFy".to_sym)')
    end
  end

  # A keyword name rides as a Symbol whatever the guest wrote it as, so a
  # String key double-splatted into a call is held to the same UTF-8 rule
  # as a Symbol one.
  def test_dispatch_non_utf8_keyword_name_is_refused
    sandbox = probe_sandbox

    assert_raises(Kobako::SandboxError,
                  "E-55: a keyword name whose bytes are not UTF-8 through a dispatch must be " \
                  "refused at the guest call site, not renamed") do
      sandbox.eval('Probe::Sink.call(**{"k\xFFy" => 1})')
    end
  end

  private

  # The Service reports what the dispatch handed it. The refusal cases
  # never reach it — that a call raises at the guest is the whole claim —
  # so they bind it without a handler.
  def probe_sandbox(&handler)
    handler ||= ->(value) { value }
    Kobako::Sandbox.new(wasm_path: REAL_WASM).tap do |sandbox|
      sandbox.bind("Probe::Sink", ->(*args, **kwargs) { handler.call(args.first || kwargs) })
    end
  end
end
