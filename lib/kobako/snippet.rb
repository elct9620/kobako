# frozen_string_literal: true

require_relative "snippet/binary"
require_relative "snippet/source"
require_relative "snippet/table"

module Kobako
  # Kobako::Snippet — namespace for the per-Sandbox preloaded snippet
  # registry and its entry value objects
  # ({docs/behavior.md B-32 / B-33}[link:../../docs/behavior.md]).
  #
  # The +Table+ owns insertion-ordered storage and seal-coordination with
  # the owning Sandbox; +Source+ is the value object representing a single
  # +#preload(code:, name:)+ entry. Entry types live as siblings under
  # this module rather than nested under +Table+ so they remain plain
  # value objects with no implicit dependency on the registry that holds
  # them.
  module Snippet
  end
end
