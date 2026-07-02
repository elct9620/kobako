# frozen_string_literal: true

require_relative "kobako/version"

begin
  RUBY_VERSION =~ /(\d+\.\d+)/
  require "kobako/#{Regexp.last_match(1)}/kobako"
rescue LoadError
  require "kobako/kobako"
end

require_relative "kobako/errors"
require_relative "kobako/transport"
require_relative "kobako/catalog"
require_relative "kobako/runtime"
require_relative "kobako/sandbox"
require_relative "kobako/pool"
