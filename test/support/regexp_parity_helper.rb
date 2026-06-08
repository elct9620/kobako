# frozen_string_literal: true

# Differential-parity harness shared by the focused Regexp / MatchData /
# String coverage classes under test/regexp/ (SPEC.md B-41).
#
# Each scenario runs against two guests: the bundled C-gem guest
# (data/kobako.wasm, Onigmo) as the behaviour oracle, and — when the
# feature-built Rust guest (data/kobako+regexp.wasm) is present — against
# it too. The covered surface mirrors the original mruby-onig-regexp gem,
# not the full CRuby Regexp API. A handful of deliberate divergences where
# the C gem returns an Onigmo internal or a bug are asserted Rust-only
# (see test/regexp/test_divergences.rb).
module RegexpParityHelper
  REAL_WASM = File.expand_path("../../data/kobako.wasm", __dir__)
  RUST_WASM = File.expand_path("../../data/kobako+regexp.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    skip "data/kobako.wasm missing — run `bundle exec rake wasm:build`" unless File.exist?(REAL_WASM)
  end

  # Evaluate +code+ in a fresh Sandbox on the guest at +wasm_path+. A fresh
  # Sandbox per scenario keeps the per-invocation match globals ($~ / $1)
  # isolated between scenarios.
  def eval_on(wasm_path, code)
    Kobako::Sandbox.new(wasm_path: wasm_path).eval(code)
  end

  # The C-gem oracle and, when the feature build is present, the Rust gem
  # must both yield +expected+ for +code+.
  def assert_parity(expected, code, message)
    assert_equal expected, eval_on(REAL_WASM, code), "C-gem oracle: #{message}"
    return unless File.exist?(RUST_WASM)

    assert_equal expected, eval_on(RUST_WASM, code), "Rust gem: #{message}"
  end

  # Both guests must yield nil for +code+ — the no-match contract, kept
  # distinct because Minitest rejects assert_equal nil.
  def assert_parity_nil(code, message)
    assert_nil eval_on(REAL_WASM, code), "C-gem oracle: #{message}"
    return unless File.exist?(RUST_WASM)

    assert_nil eval_on(RUST_WASM, code), "Rust gem: #{message}"
  end

  # Assert +code+ only against the Rust gem — for behaviours where the Rust
  # gem deliberately follows MRI instead of the C gem's Onigmo-internal or
  # buggy result. Skips until the feature build exists.
  def assert_rust_only(expected, code, message)
    skip "data/kobako+regexp.wasm missing — run `bundle exec rake wasm:build:regexp`" unless File.exist?(RUST_WASM)

    assert_equal expected, eval_on(RUST_WASM, code), "Rust gem: #{message}"
  end

  # Both guests must raise the same +error_class+ for +code+.
  def assert_parity_raises(error_class, code, message)
    assert_raises(error_class, "C-gem oracle: #{message}") { eval_on(REAL_WASM, code) }
    return unless File.exist?(RUST_WASM)

    assert_raises(error_class, "Rust gem: #{message}") { eval_on(RUST_WASM, code) }
  end

  # The Rust gem alone must raise +error_class+ for +code+ — for engine
  # behaviours (e.g. a backtracking limit) the C gem does not share. Skips
  # until the feature build exists.
  def assert_rust_raises(error_class, code, message)
    skip "data/kobako+regexp.wasm missing — run `bundle exec rake wasm:build:regexp`" unless File.exist?(RUST_WASM)

    assert_raises(error_class, "Rust gem: #{message}") { eval_on(RUST_WASM, code) }
  end
end
