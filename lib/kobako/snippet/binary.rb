# frozen_string_literal: true

module Kobako
  module Snippet
    class Binary < Data.define(:body) # :nodoc:
      # Names the snippet form the guest replays this entry as. The wire's
      # discriminant byte is assigned by the core envelope, not here.
      KIND = :bytecode
    end
  end
end
