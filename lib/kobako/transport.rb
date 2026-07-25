# frozen_string_literal: true

require_relative "transport/call"
require_relative "transport/run"
require_relative "transport/yielder"
require_relative "transport/error"
require_relative "transport/reflection"
require_relative "transport/dispatcher"

module Kobako
  # Kobako::Transport — host↔guest message transport namespace.
  # Houses the host-side call value objects (+Call+ / +Run+), the
  # guest→host +Dispatcher+, and the host→guest +Yielder+.
  # +Sandbox#initialize+ composes them onto the
  # +Runtime+ as a dispatch +Proc+ + +yield_to_guest+ lambda pair.
  # "RPC" was deliberately not chosen — it implies a cross-process boundary that
  # kobako does not have, since host and guest share one OS thread and
  # one wasm linear memory. See
  # {SPEC.md Refinement → Internal Concepts}[link:../../SPEC.md].
  module Transport
  end
end
