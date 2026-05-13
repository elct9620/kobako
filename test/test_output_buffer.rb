# frozen_string_literal: true

# Unit tests for Kobako::Sandbox::OutputBuffer.
#
# Truncate-with-marker is a SPEC.md B-04 contract owned by OutputBuffer
# itself; Sandbox's role is only to construct two buffers at the right
# limits and drain WASI pipe bytes into them after a #run. Exercise the
# overflow / clear / encoding semantics directly here so the Sandbox suite
# stays focused on lifecycle wiring (test_sandbox.rb).

require "minitest/autorun"

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "kobako/sandbox/output_buffer"

class TestOutputBuffer < Minitest::Test
  Buffer = Kobako::Sandbox::OutputBuffer

  def test_under_limit_appends_without_sealing
    buf = Buffer.new(8)
    buf << "1234567" # 7 bytes, under limit
    refute buf.truncated?
    assert_equal 7, buf.bytesize
    assert_equal "1234567", buf.to_s
  end

  def test_overflow_seals_buffer_and_appends_truncated_marker_on_read
    buf = Buffer.new(8)
    buf << "1234567"
    buf << "89AB" # would exceed by 3 bytes

    assert buf.truncated?, "buffer must seal once limit is hit"
    assert_equal 8, buf.bytesize
    assert_equal "12345678[truncated]", buf.to_s
  end

  def test_appends_after_seal_are_silently_discarded
    buf = Buffer.new(8)
    buf << "1234567"
    buf << "89AB"

    buf << "more"
    assert_equal 8, buf.bytesize
    assert_equal "12345678[truncated]", buf.to_s
  end

  def test_overflow_on_exact_byte_boundary_seals_without_writing_more
    buf = Buffer.new(4)
    buf << "abcd"
    refute buf.truncated?

    buf << "e"
    assert buf.truncated?
    assert_equal "abcd[truncated]", buf.to_s
  end

  def test_clear_resets_buffer_to_empty_and_unseals
    buf = Buffer.new(8)
    buf << "hello"
    refute buf.empty?

    buf.clear
    assert buf.empty?
    refute buf.truncated?
    assert_equal "", buf.to_s
  end

  def test_clear_after_seal_unseals_and_resets_marker
    buf = Buffer.new(4)
    buf << "abcd"
    buf << "more"
    assert buf.truncated?

    buf.clear
    buf << "ok"
    refute buf.truncated?
    assert_equal "ok", buf.to_s
  end

  def test_limit_must_be_positive_integer
    assert_raises(ArgumentError) { Buffer.new(0) }
    assert_raises(ArgumentError) { Buffer.new(-1) }
    assert_raises(ArgumentError) { Buffer.new("8") }
  end

  def test_to_s_returns_utf8_when_bytes_are_valid
    buf = Buffer.new(16)
    buf << "héllo".b
    out = buf.to_s
    assert_equal Encoding::UTF_8, out.encoding
    assert_equal "héllo", out
  end

  def test_to_s_falls_back_to_binary_when_bytes_are_not_valid_utf8
    buf = Buffer.new(8)
    buf << "\xff\xfe".b
    out = buf.to_s
    assert_equal Encoding::ASCII_8BIT, out.encoding
  end
end
