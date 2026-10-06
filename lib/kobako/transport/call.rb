# frozen_string_literal: true

module Kobako
  module Transport
    class Call < Data.define(:target, :method_name, :block_given, :payload) # :nodoc:
    end
  end
end
