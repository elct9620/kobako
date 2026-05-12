# frozen_string_literal: true

# Host-side namespace for the kobako wire contract (SPEC.md → Wire
# Contract). The wire is split into two layers, mirrored on the Rust
# side by the +codec+ / +envelope+ modules in the +kobako-wasm+ crate:
#
#   - {Codec} — byte-level MessagePack codec (SPEC.md → Wire Codec):
#     {Codec::Encoder}, {Codec::Decoder}, {Codec::Factory}, plus the
#     {Codec::Error} taxonomy. This is the layer that emits and
#     consumes raw bytes; ext types 0x01 (Capability Handle) and
#     0x02 (Exception envelope) are registered exactly once here.
#
#   - {Envelope} — logical message framing (SPEC.md → Wire Contract):
#     {Envelope::Request} / {Envelope::Response} / {Envelope::Result}
#     / {Envelope::Panic} / {Envelope::Outcome} value objects and
#     their encode/decode helpers, built on top of {Codec}.
#
# {Handle} and {Exception} are value objects that travel through both
# layers; they live directly under +Wire+ so neither layer "owns" them.
#
# The namespace is intentionally self-contained — it does not depend
# on the native extension or on +lib/kobako.rb+ — so it can be required
# directly from tests that run on a clean checkout (no compiled artifacts).
module Kobako
  module Wire
    # MessagePack ext type code reserved for Capability Handle
    # (SPEC.md → Wire Codec → Ext Types → ext 0x01).
    EXT_HANDLE = 0x01

    # MessagePack ext type code reserved for Exception envelope
    # (SPEC.md → Wire Codec → Ext Types → ext 0x02).
    EXT_ERRENV = 0x02
  end
end

require_relative "wire/handle"
require_relative "wire/exception"
require_relative "wire/codec"
require_relative "wire/envelope"
