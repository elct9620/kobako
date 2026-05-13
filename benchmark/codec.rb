# frozen_string_literal: true

# SPEC.md "Regression benchmarks" #3 — Codec throughput at varying
# payload sizes and nesting depths. SPEC explicitly requires the two
# dimensions to be measured independently and host/guest sides
# separately:
#
#   3a — fixed depth=1, varying payload size (64 B / 1 KiB / 64 KiB /
#        1 MiB). 16 MiB is gated under `BENCH_FULL=1` per the
#        smoke/full split (SPEC: payload upper bound is 16 MiB).
#   3b — fixed payload, varying nesting depth (1 / 4 / 16 / 64).
#   3c — per-wire-type micro-bench for every entry in the SPEC.md
#        Type Mapping table (12 wire types).
#
# Host side is measured directly against
# Kobako::Wire::Codec::Encoder / Decoder. Guest side is measured by
# Sandbox#run returning a constructed value — the absolute number
# bundles guest-side encode + host-side decode + the constant #run
# overhead; comparison across sizes / depths isolates the codec
# component.

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
$LOAD_PATH.unshift File.expand_path("support", __dir__)

require "kobako"
require "runner"

runner = Kobako::Bench::Runner.new("codec")

# 3a — host encode/decode, varying size at depth=1
size_bytes = { "64B" => 64, "1KiB" => 1024, "64KiB" => 64 * 1024, "1MiB" => 1024 * 1024 }
size_bytes["16MiB"] = 16 * 1024 * 1024 if ENV["BENCH_FULL"] == "1"

size_bytes.each do |label, bytes|
  payload = "x" * bytes
  encoded = Kobako::Wire::Codec::Encoder.encode(payload)
  runner.case("3a-host-encode-#{label}") { Kobako::Wire::Codec::Encoder.encode(payload) }
  runner.case("3a-host-decode-#{label}") { Kobako::Wire::Codec::Decoder.decode(encoded) }
end

# 3b — host encode/decode, varying nesting depth at fixed 1 KiB leaf
def nest(value, depth)
  depth.times { value = [value] }
  value
end

[1, 4, 16, 64].each do |depth|
  leaf = "x" * 1024
  payload = nest(leaf, depth)
  encoded = Kobako::Wire::Codec::Encoder.encode(payload)
  runner.case("3b-host-encode-depth-#{depth}") { Kobako::Wire::Codec::Encoder.encode(payload) }
  runner.case("3b-host-decode-depth-#{depth}") { Kobako::Wire::Codec::Decoder.decode(encoded) }
end

# 3c — per-wire-type micro-bench (SPEC.md Type Mapping, 12 entries).
# Handle (ext 0x01) and Exception envelope (ext 0x02) round-trip
# through the Factory just like the primitives.
sample_exception = Kobako::Wire::Exception.new(type: "runtime", message: "boom")
wire_types = {
  "nil" => nil,
  "bool" => true,
  "int" => 42,
  "float" => 3.14,
  "str" => "hello",
  "bin" => "\x00\x01\x02".b,
  "array" => [1, 2, 3],
  "map" => { "a" => 1 },
  "symbol" => :sym,
  "handle" => Kobako::Wire::Handle.new(7),
  "exception" => sample_exception
}

wire_types.each do |name, value|
  encoded = Kobako::Wire::Codec::Encoder.encode(value)
  runner.case("3c-host-encode-#{name}") { Kobako::Wire::Codec::Encoder.encode(value) }
  runner.case("3c-host-decode-#{name}") { Kobako::Wire::Codec::Decoder.decode(encoded) }
end

# 3a / 3b — guest side: Sandbox#run returning a constructed value.
# Absolute ips includes the constant Sandbox#run overhead (see
# #1 1b); per-size and per-depth ratios are the regression signal.
sandbox = Kobako::Sandbox.new
sandbox.run("nil") # warm

# Guest-side String is capped by MRB_STR_LENGTH_MAX (1 MiB; the
# parser check is `>= max`, so exactly 1 MiB raises). Cap the
# largest guest payload at 512 KiB; host-side still tests up to
# 1 MiB above.
guest_size_bytes = { "64B" => 64, "1KiB" => 1024, "64KiB" => 64 * 1024, "512KiB" => 512 * 1024 }

guest_size_bytes.each do |label, bytes|
  script = "\"x\" * #{bytes}"
  runner.case("3a-guest-return-#{label}") { sandbox.run(script) }
end

[1, 4, 16, 64].each do |depth|
  open_b = "[" * depth
  close_b = "]" * depth
  script = "#{open_b}\"x\" * 1024#{close_b}"
  runner.case("3b-guest-return-depth-#{depth}") { sandbox.run(script) }
end

puts runner.write!
