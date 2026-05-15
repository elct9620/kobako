# frozen_string_literal: true

# Host-side namespace for the RPC framing layer of the kobako wire
# contract (SPEC.md → Wire Contract). The byte-level MessagePack codec
# lives at top level under +Kobako::Codec+ (mirroring the guest-side
# +crate::codec+ module); +Wire+ owns only the RPC-path framing on top
# of that codec, mirroring the guest-side +crate::envelope+ module.
#
#   - {Envelope} — RPC-path message framing (SPEC.md → Wire Contract):
#     {Envelope::Request} / {Envelope::Response} value objects and
#     their encode/decode helpers, built on top of +Kobako::Codec+. The
#     Outcome path (success-value or Panic returned from
#     +__kobako_run+) is owned by +Kobako::Outcome+ — it does not
#     live under +Wire+.
#
# {Handle} and {Exception} are wire-level value objects shared by
# +Kobako::Codec+ (for ext-type registration) and {Envelope} (for
# Request +target+ and Response.err payload). They currently live
# under +Wire+ as a transitional placement; relocating them to their
# semantic owners (Handle → RPC Server, Exception → RPC framing) is
# part of the planned Wire→RPC umbrella restructure.
#
# The namespace is intentionally self-contained — it does not depend
# on the native extension or on +lib/kobako.rb+ — so it can be required
# directly from tests that run on a clean checkout (no compiled artifacts).
module Kobako
  # See the file-level documentation above. The module body is
  # intentionally empty: the logical framing lives in {Wire::Envelope}
  # and the shared value objects ({RPC::Handle} / {RPC::Fault})
  # load themselves into this namespace via the +require_relative+
  # calls below. The byte-level codec lives at +Kobako::Codec+ and is
  # pulled in transitively by +wire/envelope+.
  module Wire
  end
end

require_relative "rpc/handle"
require_relative "rpc/fault"
require_relative "wire/envelope"
