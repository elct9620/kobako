# frozen_string_literal: true

module Kobako
  # Kobako::Unresolved — the sentinel backing a fillable Service path: one
  # declared with +Sandbox#bind(path)+ carrying no object. It reserves the
  # path's Frame 1 slot so the guest sees the bound constant while the host
  # defers the object it stands for. A guest dispatch to an unfilled fillable
  # is refused as an unresolved target and surfaces as +Kobako::ServiceError+
  # when the guest leaves it unrescued — the same capability-failure channel
  # as an idiom with no backend bound.
  #
  # A single shared value; only its identity distinguishes it, so the dispatch
  # layer recognises it with +equal?+. A Host App may name it at a +bind+ site
  # to declare a fillable explicitly.
  module Unresolved
  end
end
