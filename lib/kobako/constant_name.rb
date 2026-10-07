# frozen_string_literal: true

module Kobako
  # A bind-path segment, a snippet name and a #run entrypoint all take the
  # constant-name form, so they are refused alike.
  CONSTANT_NAME = /\A[A-Z]\w*\z/ # :nodoc:
end
