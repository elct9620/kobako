# frozen_string_literal: true

# Shared setup for the focused Regexp / MatchData / String coverage classes
# under test/e2e/regexp/. The kobako-regexp gem is opt-in, so
# its surface lives only in the variant Guest Binaries — these scenarios drive
# the unicode variant (the full curated surface) and assert kobako-regexp's
# specified contract directly: byte-based offsets, the curated method
# surface, and the MRI-aligned option / global semantics.
module RegexpGuestHelper
  include GuestGuard

  REGEXP_WASM = TestPaths.data("kobako+regexp-unicode.wasm")

  def setup
    require_guest_binary!(REGEXP_WASM, build: "bundle exec rake wasm:build:regexp_unicode")
  end

  # Evaluate +code+ in a fresh Sandbox on the regexp guest. A fresh Sandbox
  # per scenario keeps the per-invocation match globals ($~ / $1) isolated
  # between scenarios.
  def eval_regexp(code)
    Kobako::Sandbox.new(wasm_path: REGEXP_WASM).eval(code).value
  end

  # Evaluate +code+ expecting it to raise +expected+ (a guest exception
  # class name): returns that name on the expected raise, the actual class
  # name on any other raise, and +"no-error"+ when nothing raises — so an
  # assertion failure names what really happened.
  def guard_error(code, expected)
    eval_regexp("begin; #{code}; 'no-error'; " \
                "rescue #{expected}; #{expected.inspect}; " \
                "rescue => e; e.class.to_s; end")
  end
end
