# frozen_string_literal: true

require "test_helper"

# The Unicode gate distinguishes the two regexp variants (SPEC.md B-41).
# fancy-regex's Unicode support is coarse: with it off the engine rejects
# every case-insensitive pattern. These scenarios drive the no-unicode
# +regexp+ Guest Binary to pin that the gate is closed there — a guest that
# needs +/i+ must pick the unicode variant — while ASCII matching, which is
# rewritten to explicit classes regardless, still works.
class TestRegexpUnicodeGate < Minitest::Test
  REGEXP_WASM = File.expand_path("../../data/kobako+regexp.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    return if File.exist?(REGEXP_WASM)

    skip "data/kobako+regexp.wasm missing — run `bundle exec rake wasm:build:regexp`"
  end

  # A case-insensitive pattern must raise RegexpError on the no-unicode
  # variant rather than silently matching case-sensitively. The pattern is
  # used (mruby elides a discarded bare literal) so its compilation runs.
  def test_case_insensitive_pattern_is_rejected_without_unicode
    result = eval_no_unicode(
      "begin; /foo/i.match('x'); 'no-error'; rescue RegexpError; 'RegexpError'; rescue => e; e.class.to_s; end"
    )

    assert_equal "RegexpError", result,
                 "a /i pattern through #eval on the no-unicode variant must raise RegexpError"
  end

  # ASCII shorthand classes are rewritten to explicit ranges either way, so
  # plain matching stays available without the unicode feature.
  def test_ascii_matching_works_without_unicode
    result = eval_no_unicode('/\d+/.match("abc123")[0]')

    assert_equal "123", result,
                 "an ASCII \\d pattern through #eval on the no-unicode variant must still match"
  end

  private

  def eval_no_unicode(code)
    Kobako::Sandbox.new(wasm_path: REGEXP_WASM).eval(code)
  end
end
