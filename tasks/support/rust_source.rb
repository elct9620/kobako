# frozen_string_literal: true

# The Rust source-shape rule shared by the pub-surface and hotspot
# instruments — one definition of where a file's implementation ends,
# so the two scans cannot drift apart on what a test tail is.
module KobakoRustSource
  module_function

  # A +#[cfg(test)]+ gate that opens a test module — the only shape that
  # truncates a scan; an inline cfg(test) item must not hide the
  # implementation that follows it.
  TEST_MODULE = /^\s*#\[cfg\(test\)\]\s*\n\s*mod\b/

  # Everything before the +#[cfg(test)]+ tail module (test modules sit
  # at the end of a file by convention); the whole text when a file
  # carries no tail.
  def impl_body(text)
    text.split(TEST_MODULE, 2).first
  end
end
