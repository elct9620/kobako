# frozen_string_literal: true

require_relative "payload/arguments"

module Kobako
  # The payload shapes that ride inside a core envelope's opaque payload
  # field. The envelope routes a message and the native side decodes it;
  # this layer decides what the resolved method receives, and the byte-level
  # encoding of that shape belongs to +Kobako::Codec+ beneath it.
  #
  # Keeping the two apart is what lets an endpoint with its own schema
  # replace this layer and keep the envelope, so nothing here may reach for
  # a routing field.
  #
  # +Kobako::Payload::Arguments+ is the wire-symmetric peer of
  # +kobako_codec::payload::Arguments+.
  module Payload
  end
end
