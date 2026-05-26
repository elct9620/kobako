# frozen_string_literal: true

require_relative "transport/request"
require_relative "transport/response"
require_relative "transport/run"
require_relative "transport/yield"
require_relative "transport/yielder"
require_relative "transport/wire_error"
require_relative "transport/dispatcher"

module Kobako
  # Kobako::Transport — host↔guest message transport namespace.
  # Houses the envelope value objects (Request / Response / Run / Yield),
  # the guest→host +Dispatcher+, and the host→guest +Yielder+.
  # +Sandbox#initialize+ composes them onto the
  # +Runtime+ as a dispatch +Proc+ + +yield_to_guest+ lambda pair
  # ({docs/behavior.md B-12}[link:../../docs/behavior.md]). "RPC" was
  # deliberately not chosen — it implies a cross-process boundary that
  # kobako does not have, since host and guest share one OS thread and
  # one wasm linear memory. See
  # {SPEC.md Refinement → Internal Concepts}[link:../../SPEC.md].
  module Transport
  end
end
