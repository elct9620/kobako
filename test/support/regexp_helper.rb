# frozen_string_literal: true

# Shared setup for the focused Regexp / MatchData / String coverage classes
# under test/regexp/ (SPEC.md B-41). Each scenario evaluates guest code on
# the regexp-enabled Guest Binary and asserts kobako-regexp's specified
# contract directly: byte-based offsets, the curated method surface, and the
# MRI-aligned option / global semantics.
module RegexpGuestHelper
  REGEXP_WASM = File.expand_path("../../data/kobako.wasm", __dir__)

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Runtime)
    skip "data/kobako.wasm missing — run `bundle exec rake wasm:build`" unless File.exist?(REGEXP_WASM)
  end

  # Evaluate +code+ in a fresh Sandbox on the regexp guest. A fresh Sandbox
  # per scenario keeps the per-invocation match globals ($~ / $1) isolated
  # between scenarios.
  def eval_regexp(code)
    Kobako::Sandbox.new(wasm_path: REGEXP_WASM).eval(code)
  end
end
