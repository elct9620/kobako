# frozen_string_literal: true

module Kobako
  # Kobako::Transport — host↔guest message transport namespace.
  # Houses the envelope value objects (Request / Response / Fault / Run /
  # Yield), the guest→host +Dispatcher+, and the host→guest
  # +YieldProxy+ factory. +Sandbox#initialize+ composes them onto the
  # +Runtime+ as a dispatch +Proc+ + +yield_to_guest+ lambda pair
  # ({BRIDGE_REDESIGN §5.5.3}). "RPC" was deliberately not chosen — it
  # implies a cross-process boundary that kobako does not have, since
  # host and guest share one OS thread and one wasm linear memory. See
  # {SPEC.md Refinement → Internal Concepts}[link:../../SPEC.md].
  module Transport
  end
end
