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

  # Witnesses the mid-file shape the corpus actually holds:
  # kobako-wasmtime's invocation.rs carries an inline +#[cfg(test)]+
  # constructor above its tail test module, and the pub items that
  # follow it are still public surface.
  def test_pub_items_survive_an_inline_cfg_test_item
    sources = { "src/invocation.rs" => <<~RS }
      #[cfg(test)]
      pub(crate) fn new(desired: usize, limit: usize) -> Self {}
      pub fn after_inline_gate() {}
      #[cfg(test)]
      mod tests {
          pub fn helper_in_tests() {}
      }
    RS

    assert_equal [["after_inline_gate", "src/invocation.rs:3"]], Surface.pub_items(sources),
                 "an inline cfg(test) item must not hide the public items that follow it"
  end

  def test_unconsumed_excludes_referenced_and_acknowledged_items
    items = [["pack_u64", "src/abi.rs:1"], ["take_outcome", "src/abi.rs:2"], ["orphan", "src/abi.rs:3"]]
    consumers = "let word = pack_u64(p, l);"

    unconsumed = Surface.unconsumed(items, consumers, acknowledged: { "take_outcome" => "macro-expanded" })

    assert_equal [["orphan", "src/abi.rs:3"]], unconsumed
  end

  # The staleness half of the ledger, mirroring the Pending-anchors
  # rule: an acknowledgement whose pub item is gone is dead weight the
  # ledger must shed.
  def test_stale_acknowledgements_list_entries_no_pub_item_carries
    items = [["pack_u64", "src/abi.rs:1"]]

    stale = Surface.stale_acknowledgements(items, { "pack_u64" => "kept", "renamed_away" => "kept" })

    assert_equal ["renamed_away"], stale,
                 "an acknowledged name no current pub item carries must surface as stale"
  end

  # The scope-drift guard: a crate directory that is neither analyzed,
  # consuming, nor exempt means the scan silently lags the repo.
  def test_unaccounted_crates_lists_only_unclassified_roster_entries
    unaccounted = Surface.unaccounted_crates(
      roster: ["crates/a", "crates/b", "wasm/c", "wasm/d"],
      analyzed: ["crates/a"],
      consumers: ["wasm/c"],
      exempt: ["wasm/d"]
    )

    assert_equal ["crates/b"], unaccounted,
                 "a roster crate with no analyzed/consumer/exempt classification must surface as drift"
  end

  def test_unconsumed_requires_a_word_boundary_match
    items = [["pack", "src/abi.rs:1"]]

    assert_empty Surface.unconsumed(items, "pack(1, 2)"),
                 "a call site must count as consumption"
    assert_equal items, Surface.unconsumed(items, "unpack_u64(word)"),
                 "a longer identifier containing the name must not count as consumption"
  end
end
