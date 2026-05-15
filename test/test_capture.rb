# frozen_string_literal: true

require "test_helper"

# Kobako::Capture is the immutable value object that pairs the host-side
# captured prefix of guest stdout / stderr with the truncation flag the
# WASI pipe sets when the guest wrote past the configured cap
# (SPEC.md B-04). Sandbox stores one Capture per channel between runs.
class TestCapture < Minitest::Test
  def test_empty_constant_carries_utf8_empty_bytes
    assert_equal "", Kobako::Capture::EMPTY.bytes
    assert_equal Encoding::UTF_8, Kobako::Capture::EMPTY.bytes.encoding
  end

  def test_empty_constant_is_not_truncated
    refute Kobako::Capture::EMPTY.truncated?
  end

  def test_empty_constant_is_frozen
    assert_predicate Kobako::Capture::EMPTY, :frozen?
  end

  def test_initialize_exposes_bytes_and_truncated_predicate
    capture = Kobako::Capture.new(bytes: "hello", truncated: true)

    assert_equal "hello", capture.bytes
    assert_predicate capture, :truncated?
  end

  def test_instances_are_frozen_after_initialize
    capture = Kobako::Capture.new(bytes: "data", truncated: false)

    assert_predicate capture, :frozen?
  end

  # SPEC.md B-04: ext provides binary bytes; Capture.from_ext coerces them
  # to UTF-8 when valid so callers receive an inspectable String without
  # encoding work.
  def test_from_ext_returns_utf8_when_bytes_are_valid_utf8
    capture = Kobako::Capture.from_ext("hello".b, false)

    assert_equal Encoding::UTF_8, capture.bytes.encoding
    assert_equal "hello", capture.bytes
    refute_predicate capture, :truncated?
  end

  # Invalid UTF-8 must not raise — fall back to ASCII-8BIT so the host
  # can still inspect the raw bytes for debugging.
  def test_from_ext_falls_back_to_ascii_8bit_on_invalid_utf8
    invalid = "\xff\xfe".b
    capture = Kobako::Capture.from_ext(invalid, true)

    assert_equal Encoding::ASCII_8BIT, capture.bytes.encoding
    assert_equal invalid, capture.bytes
    assert_predicate capture, :truncated?
  end

  def test_from_ext_does_not_mutate_input_bytes
    original = "data".b

    Kobako::Capture.from_ext(original, false)

    assert_equal Encoding::ASCII_8BIT, original.encoding
  end
end
