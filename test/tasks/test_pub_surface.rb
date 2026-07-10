# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/pub_surface"

# Unit coverage for the pub-surface reader: extraction takes only truly
# public items outside the cfg(test) tail, and the unconsumed filter
# honors both a downstream word-boundary reference and the
# acknowledgement ledger.
class KobakoPubSurfaceTest < Minitest::Test
  Surface = KobakoPubSurface

  def test_pub_items_skip_crate_visibility_and_the_test_tail
    sources = { "src/abi.rs" => <<~RS }
      pub fn pack_u64(ptr: u32, len: u32) -> u64 {}
      pub(crate) fn internal_only() {}
      #[cfg(test)]
      mod tests {
          pub fn helper_in_tests() {}
      }
    RS

    assert_equal [["pack_u64", "src/abi.rs:1"]], Surface.pub_items(sources),
                 "pub(crate) items and test-module helpers are not public surface"
  end

  # Witnesses the two qualifier shapes the corpus actually holds:
  # kobako-mruby's raise helpers are +pub unsafe fn+, and a +pub const
  # fn+ must yield the function name, never the +fn+ keyword.
  def test_pub_items_read_through_fn_qualifiers
    sources = { "src/runtime.rs" => <<~RS }
      pub unsafe fn resolve_raw(mrb: &Mrb) -> Self {}
      pub const fn packed_len() -> usize {}
      pub const MAX_DEPTH: usize = 128;
    RS

    expected = [["resolve_raw", "src/runtime.rs:1"], ["packed_len", "src/runtime.rs:2"],
                ["MAX_DEPTH", "src/runtime.rs:3"]]

    assert_equal expected, Surface.pub_items(sources),
                 "a qualified pub fn must surface under its own name alongside plain const items"
  end

  def test_unconsumed_excludes_referenced_and_acknowledged_items
    items = [["pack_u64", "src/abi.rs:1"], ["take_outcome", "src/abi.rs:2"], ["orphan", "src/abi.rs:3"]]
    consumers = "let word = pack_u64(p, l);"

    unconsumed = Surface.unconsumed(items, consumers, acknowledged: { "take_outcome" => "macro-expanded" })

    assert_equal [["orphan", "src/abi.rs:3"]], unconsumed
  end

  def test_unconsumed_requires_a_word_boundary_match
    items = [["pack", "src/abi.rs:1"]]

    assert_empty Surface.unconsumed(items, "pack(1, 2)"),
                 "a call site must count as consumption"
    assert_equal items, Surface.unconsumed(items, "unpack_u64(word)"),
                 "a longer identifier containing the name must not count as consumption"
  end
end
