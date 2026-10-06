# frozen_string_literal: true

require_relative "payload/arguments"

module Kobako
  # What the resolved method receives, kept apart from the envelope so an
  # endpoint with its own schema can replace it; nothing here reads a
  # routing field.
  module Payload # :nodoc:
  end
end
