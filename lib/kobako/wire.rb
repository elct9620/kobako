# frozen_string_literal: true

# Host-side namespace for the kobako wire contract (SPEC.md → Wire
# Contract). The wire is split into two layers, mirrored on the Rust
# side by the +codec+ / +envelope+ modules in the +kobako-wasm+ crate:
#
#   - {Codec} — byte-level MessagePack codec (SPEC.md → Wire Codec):
#     {Codec::Encoder}, {Codec::Decoder}, {Codec::Factory}, plus the
#     {Codec::Error} taxonomy. This is the layer that emits and
#     consumes raw bytes; ext types 0x01 (Capability Handle) and
#     0x02 (Exception envelope) are registered exactly once on the
#     Factory, where the numeric codes live as module-private constants
#     alongside the Rust-side +codec::EXT_HANDLE+ / +codec::EXT_ERRENV+.
#
#   - {Envelope} — logical message framing (SPEC.md → Wire Contract):
#     {Envelope::Request} / {Envelope::Response} / {Envelope::Panic} /
#     {Envelope::Outcome} value objects and their encode/decode
#     helpers, built on top of {Codec}. The Result envelope has no
#     value object — its wire form is the bare msgpack encoding of
#     the returned value (no enclosing array), so the encode/decode
#     pair operates directly on the value.
#
# {Handle} and {Exception} are value objects that travel through both
# layers; they live directly under +Wire+ so neither layer "owns" them.
#
# The namespace is intentionally self-contained — it does not depend
# on the native extension or on +lib/kobako.rb+ — so it can be required
# directly from tests that run on a clean checkout (no compiled artifacts).
module Kobako
  # See the file-level documentation above for the layer split. The
  # module body is intentionally empty: the byte-level codec lives in
  # {Wire::Codec}, the logical framing in {Wire::Envelope}, and the
  # shared value objects ({Wire::Handle} / {Wire::Exception}) load
  # themselves into this namespace via the +require_relative+ calls
  # below.
  module Wire
  end
end

require_relative "wire/handle"
require_relative "wire/exception"
require_relative "wire/codec"
require_relative "wire/envelope"
