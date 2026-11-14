# frozen_string_literal: true

require_relative "catalog/handles"
require_relative "catalog/services"
require_relative "catalog/snippets"
require_relative "catalog/flags"
require_relative "catalog/extensions"
require_relative "catalog/mount"
require_relative "catalog/network"
require_relative "catalog/grants"
require_relative "catalog/replacements"

module Kobako
  module Catalog # :nodoc:
    # A registered name is a Symbol or a String, as CRuby's +const_get+ and
    # +define_method+ take one; any other class is refused before its text
    # is read.
    def self.to_name(name, what)
      unless name.is_a?(Symbol) || name.is_a?(String)
        raise TypeError,
              "#{what} must be a Symbol or String, got #{name.class}"
      end

      name.to_s
    end

    # The native side carries a registered text only as UTF-8, so text read
    # as bytes is re-tagged, and bytes that are not UTF-8 are refused at
    # registration rather than failing every invocation.
    def self.to_utf8(text, what)
      utf8 = text.dup.force_encoding(Encoding::UTF_8)
      raise ArgumentError, "#{what} must be UTF-8 text" unless utf8.valid_encoding?

      utf8
    end
  end
end
