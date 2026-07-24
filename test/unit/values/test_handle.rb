# frozen_string_literal: true

require "test_helper"

# Kobako::Handle's construction surface is deliberately closed: the wire
# contract (docs/wire-contract.md § Capability Handle, "Not constructible
# by guest or Host App") pins that the Host App has no public path that
# turns a bare integer into a Handle. `.new` is privatised, and `#with` —
# Data's copy-with-changes constructor, which would let a legitimate
# Handle (reaching Host App code as an error field) mint a sibling with a
# caller-chosen id — is removed. `.restore` stays as the Host Gem-internal
# factory; its range invariants are pinned in test/codec/test_ext_types.rb.
class TestHandle < Minitest::Test
  def test_new_is_not_a_public_constructor
    assert_raises(NoMethodError, "a bare integer through Kobako::Handle.new must not construct a Handle") do
      Kobako::Handle.new(id: 1)
    end
  end

  def test_with_cannot_derive_a_handle_with_a_chosen_id
    handle = Kobako::Handle.restore(1)
    assert_raises(NoMethodError, "a legitimate Handle through #with must not mint a Handle with a caller-chosen id") do
      handle.with(id: 999)
    end
  end
end
