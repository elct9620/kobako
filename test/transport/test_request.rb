# frozen_string_literal: true

require "test_helper"

# Unit + integration tests for the host-side Request envelope encoder /
# decoder (SPEC.md F-05 / F-09), mirroring lib/kobako/transport/request.rb.
# Builds on top of the byte-level wire codec covered by test/codec/; no
# native extension dependency. The Response side lives in test_response.rb.
class TestTransportRequest < Minitest::Test
  include CodecHelpers

  Envelope = Kobako::Transport

  def test_request_construction_validates_field_types
    base = { target: "G::M", method_name: "x" }
    overrides = [
      { target: 123 }, { method_name: :sym }, { args: "no" },
      { kwargs: [] }, { block_given: "true" }
    ]
    overrides.each do |o|
      assert_raises(ArgumentError, "a wrong-typed #{o.keys.first} through Request.new must raise ArgumentError") do
        Envelope::Request.new(**base, **o)
      end
    end
  end

  def test_request_block_given_defaults_to_false
    req = Envelope::Request.new(target: "G::M", method_name: "ping")
    refute req.block_given, "an omitted block_given through Request.new must default to false"
  end

  def test_request_round_trip_with_block_given_true
    req = Envelope::Request.new(
      target: "Each::Iter",
      method_name: "run",
      args: [[1, 2, 3]],
      kwargs: {},
      block_given: true
    )
    decoded = Envelope::Request.decode(req.encode)
    assert_equal req, decoded, "a block_given Request must survive an encode/decode round-trip unchanged"
    assert decoded.block_given, "block_given: true must round-trip as true"
  end

  def test_request_round_trip_with_string_target
    req = Envelope::Request.new(
      target: "Store::Users",
      method_name: "find",
      args: [42, "alice"],
      kwargs: { active: true }
    )
    bytes   = req.encode
    decoded = Envelope::Request.decode(bytes)
    assert_equal req, decoded, "a String-target Request must survive an encode/decode round-trip unchanged"
  end

  def test_request_round_trip_with_handle_target
    req = Envelope::Request.new(
      target: Handle.restore(7),
      method_name: "save",
      args: [],
      kwargs: {}
    )
    bytes   = req.encode
    decoded = Envelope::Request.decode(bytes)
    assert_equal req, decoded, "a Handle-target Request must survive an encode/decode round-trip unchanged"
    assert_instance_of Handle, decoded.target, "a Handle target must decode back to a Kobako::Handle, not a raw id"
  end

  def test_request_handles_in_args
    h1 = Handle.restore(1)
    h2 = Handle.restore(2)
    req = Envelope::Request.new(
      target: "G::M",
      method_name: "link",
      args: [h1, h2, "tag"],
      kwargs: { k: h1 }
    )
    decoded = Envelope::Request.decode(req.encode)
    assert_equal req, decoded, "Handles bare and nested in args/kwargs must survive the round-trip unchanged"
  end

  def test_request_kwargs_must_have_symbol_keys
    # SPEC.md → Wire Codec → Ext Types → ext 0x00 pins kwargs keys
    # to Symbols. The Value Object refuses non-Symbol kwargs keys at
    # construction; the wire-level InvalidType guarantee is preserved
    # via the Request.decode boundary translator.
    assert_raises(ArgumentError, "a non-Symbol kwargs key through Request.new must raise ArgumentError") do
      Envelope::Request.new(target: "G::M", method_name: "x", args: [], kwargs: { "active" => true })
    end
  end

  def test_request_decode_translates_non_symbol_kwargs_key_to_invalid_type
    # Forge wire bytes with an int kwargs key — msgpack-legal but
    # envelope-illegal. The decoder must translate the value-object
    # ArgumentError into a wire-layer InvalidType.
    bytes = Encoder.encode(["G::M", "x", [], { 42 => "v" }, false])
    assert_raises(InvalidType, "decoding a Request with a non-Symbol kwargs key must surface as a wire InvalidType") do
      Envelope::Request.decode(bytes)
    end
  end

  def test_request_decode_rejects_wrong_arity
    # 4-element array, not 5 — post-B-23 the Request envelope carries
    # +block_given+ as the 5th element.
    bytes = Encoder.encode(["G::M", "x", [], {}])
    assert_raises(InvalidType, "decoding a 4-element Request envelope must be rejected as a wire InvalidType") do
      Envelope::Request.decode(bytes)
    end
  end

  def test_request_decode_rejects_trailing_bytes
    # An envelope payload is exactly one msgpack value (docs/wire-codec.md
    # § Envelope Encoding); a second value after it signals framing desync.
    bytes = Encoder.encode(["G::M", "x", [], {}, false]) + Encoder.encode(nil)
    assert_raises(InvalidType,
                  "decoding a Request envelope with trailing bytes must be rejected as a wire InvalidType") do
      Envelope::Request.decode(bytes)
    end
  end

  # ---------- Request golden vector ----------

  def test_request_golden_empty_args_and_kwargs
    # Request: ["G::M", "ping", [], {}, false]
    # fixarray 5 (0x95) | fixstr 4 "G::M" (0xa4 47 3a 3a 4d) |
    # fixstr 4 "ping" (0xa4 70 69 6e 67) | fixarray 0 (0x90) |
    # fixmap 0 (0x80) | false (0xc2)
    bytes = Envelope::Request.new(target: "G::M", method_name: "ping").encode
    assert_equal "95a4473a3a4da470696e679080c2", hex(bytes),
                 "Request#encode must produce the canonical SPEC byte sequence for the empty-args/kwargs shape"
  end
end
