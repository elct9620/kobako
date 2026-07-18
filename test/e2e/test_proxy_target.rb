# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — guest→host dispatch target derivation through real mruby.
# The Kobako::Proxy seam derives a Request target from the receiver's exact
# identity: an exact Kobako::Handle by its id, a class by its constant path.
# A receiver that mixed in the module without being either has no target and
# is refused in-guest, emitting no wire Request (B-59). The positive paths are
# pinned by B-12 (bound constant) and B-17 (Handle) elsewhere.
class TestE2EProxyTarget < Minitest::Test
  include E2eGuestHelper

  # A guest class that mixes in Kobako::Proxy is neither an exact
  # Kobako::Handle nor a class, so method_missing finds no target and refuses
  # before any wire Request; uncaught it surfaces as SandboxError (E-04).
  def test_b59_foreign_proxy_holder_is_refused_in_guest
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Lookup", ->(key) { "value:#{key}" })

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval("class Rogue; include Kobako::Proxy; end; Rogue.new.lookup(:x)")
    end
    assert_equal "NoMethodError", err.klass,
                 "B-59: a guest object that mixed in Kobako::Proxy without being a Handle must be refused in-guest"
    assert_match(/not a Kobako dispatch target/, err.message,
                 "B-59: the in-guest refusal must name the missing-target reason")
  end
end
