# frozen_string_literal: true

require_relative "transport/call"
require_relative "transport/run"
require_relative "transport/yielder"
require_relative "transport/error"
require_relative "transport/exposure"
require_relative "transport/reflection"
require_relative "transport/dispatcher"

module Kobako
  # The host side of the guest↔host exchange. Only Transport::Error is
  # public; the rest serves one invocation's dispatch.
  module Transport # :nodoc:
  end
end
