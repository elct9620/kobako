# frozen_string_literal: true

module Kobako
  module Snippet
    class Source < Data.define(:name, :body) # :nodoc:
      # Names the snippet form the guest replays this entry as. The wire's
      # discriminant byte is assigned by the core envelope, not here.
      KIND = :source
    end
  end
end
