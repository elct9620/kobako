# frozen_string_literal: true

require_relative "payload/arguments"

module Kobako
  # The MessagePack payload adapter — what rides inside a core envelope's
  # opaque payload field. The envelope routes a message and the native side
  # decodes it; this layer decides what the resolved method receives.
  #
  # Keeping the two apart is what lets an endpoint with its own schema
  # replace this layer and keep the envelope, so nothing here may reach for
  # a routing field. It is also why a large payload still decodes through
  # the MessagePack gem: its strings stay shared with the buffer the ext
  # handed over rather than copied out of it.
  #
  # +Kobako::Payload::Arguments+ is the wire-symmetric peer of
  # +kobako_codec::payload::Arguments+.
  module Payload
  end
end
