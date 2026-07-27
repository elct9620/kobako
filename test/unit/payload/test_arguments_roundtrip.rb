# frozen_string_literal: true

# Cross-language payload-codec round-trip (SPEC.md F-05 / F-09).
#
# Drives the Rust `payload_oracle` subprocess from the host: each test
# Ruby-encodes one codec payload, prefixes a single-byte kind tag, and
# asks the oracle to decode + re-encode it. The Ruby side then asserts
# byte-identical round-trip — proving the two codec peers agree on the
# argument shape, not just the underlying msgpack codec already covered
# by test/fuzz/test_roundtrip_fuzz.rb.
#
# The core envelope has no case here: both its peers are Rust, so their
# agreement is pinned directly by crates/kobako-runtime/tests/envelope_oracle.rs.
#
# This test does NOT need fuzz scale: a handful of representative
# payloads is enough; the codec fuzz covers byte-level wire shapes
# underneath.

require "test_helper"

class TestArgumentsRoundtrip < Minitest::Test
  CRATE_DIR = TestPaths.source("wasm", "kobako-wasm")
  ORACLE    = CargoOracle.new(crate_dir: CRATE_DIR, bin_name: "payload_oracle")

  def setup
    case (build = ORACLE.ensure_built).status
    when :no_cargo
      skip "cargo not on PATH; the payload oracle requires a Rust toolchain"
    when :build_failed
      flunk "cargo build --release payload_oracle failed:\n#{build.error}"
    end
    @channel = ORACLE.spawn
  end

  def teardown
    @channel&.close
  end

  # Send one payload frame to the oracle and read its response. +kind+ is
  # a single-byte tag picked by the oracle protocol ('A' invocation
  # Arguments).
  def oracle_roundtrip(kind, payload)
    @channel.send_frame(+"".b << kind << payload.b)
    body, error = @channel.read_frame
    flunk "oracle reported error: #{body}" if error
    body
  end

  # ---------- invocation Arguments payload ----------

  def test_invocation_arguments_round_trip
    bytes = Kobako::Payload::Arguments.new(args: [42, "alice"], kwargs: { active: true }).encode
    assert_equal bytes, oracle_roundtrip("A", bytes),
                 "an args-and-kwargs payload must survive the guest codec byte-identically"
  end

  def test_invocation_arguments_carrying_a_wrapped_leaf_round_trip
    # A non-wire-representable argument auto-wraps into the Handles table
    # and rides as ext 0x01 in its args position — the payload Handle
    # position docs/wire/payload-msgpack.md § ext 0x01 licenses.
    wrapped = Kobako::Codec::HandleWalk.deep_wrap([Object.new], Kobako::Catalog::Handles.new)
    bytes = Kobako::Payload::Arguments.new(args: wrapped, kwargs: {}).encode
    assert_equal bytes, oracle_roundtrip("A", bytes),
                 "a Handle in an argument position must cross to the guest codec unchanged"
  end

  # The empty-args/kwargs payload shape is byte-pinned cross-language by
  # the Rust golden vectors, so no oracle case here.
end
