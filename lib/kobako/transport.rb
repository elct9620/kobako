# frozen_string_literal: true

require_relative "transport/call"
require_relative "transport/run"
require_relative "transport/yielder"
require_relative "transport/error"
require_relative "transport/reflection"
require_relative "transport/dispatcher"

module Kobako
  # Kobako::Transport — host↔guest message transport namespace. Houses the
  # host side of one Call/Reply exchange: the call value objects +Call+
  # (guest→host, as the native side decoded it) and +Run+ (host→guest), the
  # +Dispatcher+ that answers a routed Call, the +Yielder+ that re-enters
  # the guest for a block, the +Reflection+ floor a dispatch must clear, and
  # +Error+ for a wire violation the host detects. Each invocation's
  # +Context+ composes them into the dispatch +Proc+ it passes +Runtime+ for
  # that run.
  #
  # "RPC" was deliberately not chosen — it implies a cross-process boundary that
  # kobako does not have, since host and guest share one OS thread and
  # one wasm linear memory. See
  # {SPEC.md Refinement → Internal Concepts}[link:../../SPEC.md].
  module Transport
  end
end
