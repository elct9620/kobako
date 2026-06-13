# frozen_string_literal: true

require_relative "snippet/binary"
require_relative "snippet/source"

module Kobako
  # Kobako::Snippet — value-object family for preloaded snippet entries
  # held by +Kobako::Catalog::Snippets+.
  #
  # +Source+ represents a single +#preload(code:, name:)+ entry; +Binary+
  # represents a single +#preload(binary:)+ entry. Both are plain value
  # objects with no dependency on the +Catalog::Snippets+ registry that
  # holds them — the registry reads their attributes externally when
  # encoding the wire envelope.
  module Snippet
  end
end
