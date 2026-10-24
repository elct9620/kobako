# frozen_string_literal: true

require_relative "catalog/handles"
require_relative "catalog/services"
require_relative "catalog/snippets"
require_relative "catalog/flags"
require_relative "catalog/extensions"
require_relative "catalog/mount"
require_relative "catalog/grants"

module Kobako
  module Catalog # :nodoc:
    # The native side carries a registered text only as UTF-8, so text read
    # as bytes is re-tagged, and bytes that are not UTF-8 are refused at
    # registration rather than failing every invocation.
    def self.to_utf8!(text, what)
      utf8 = text.dup.force_encoding(Encoding::UTF_8)
      raise ArgumentError, "#{what} must be UTF-8 text" unless utf8.valid_encoding?

      utf8
    end
  end
end
