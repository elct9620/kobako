# frozen_string_literal: true

module Kobako
  # A bind-path segment, a snippet name and a #run entrypoint all take the
  # constant-name form, so they are refused alike. The guest interns each
  # one, and mruby holds a symbol of at most 65534 bytes.
  CONSTANT_NAME = /\A[A-Z]\w{0,65533}\z/ # :nodoc:
end
