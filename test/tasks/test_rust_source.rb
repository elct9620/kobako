# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/rust_source"

# Unit coverage for the shared Rust source-shape rule: the inline
# +#[cfg(test)]+ tail module is test weight, and the instruments that
# read implementation (pub-surface inventory, hotspot sizing) must
# agree on where it starts.
class KobakoRustSourceTest < Minitest::Test
  Source = KobakoRustSource

  def test_impl_body_stops_at_the_test_tail_module
    text = <<~RS
      pub fn shipped() {}
      #[cfg(test)]
      mod tests {
          fn helper() {}
      }
    RS

    assert_equal "pub fn shipped() {}\n", Source.impl_body(text),
                 "a Rust file through impl_body must keep only the lines before its cfg(test) tail module"
  end

  def test_impl_body_of_a_tailless_file_is_the_whole_text
    text = "pub fn shipped() {}\n"

    assert_equal text, Source.impl_body(text),
                 "a Rust file with no test tail through impl_body must pass through whole"
  end

  # Witnesses the mid-file shape the corpus actually holds
  # (kobako-wasmtime's invocation.rs): an inline cfg(test) item gates
  # one item, never the rest of the file.
  def test_impl_body_reads_past_an_inline_cfg_test_item
    text = <<~RS
      #[cfg(test)]
      pub(crate) fn new() -> Self {}
      pub fn shipped() {}
    RS

    assert_equal text, Source.impl_body(text),
                 "an inline cfg(test) item through impl_body must not truncate the implementation after it"
  end
end
