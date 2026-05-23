# frozen_string_literal: true

module Kobako
  # Kobako::Transport — host↔guest message transport namespace.
  # Houses the envelope value objects (Request / Response / Fault / Run /
  # Yield), the guest→host Dispatcher, the host→guest yield re-entry
  # proxy, and the +Channel+ composition root that wires them into a
  # +Sandbox+. Replaces the former +Kobako::RPC+ namespace; "RPC" implies
  # a cross-process boundary that kobako does not have — host and guest
  # share one OS thread and one wasm linear memory. See
  # {SPEC.md Refinement → Internal Concepts}[link:../../SPEC.md].
  module Transport
  end
end
