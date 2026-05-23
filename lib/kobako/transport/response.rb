# frozen_string_literal: true

require_relative "../codec"
require_relative "../transport"
require_relative "fault"
require_relative "request"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns the Response value object and its encode/decode helpers.
  module Transport
    # Value object for a single host-side Transport Response
    # ({docs/wire-codec.md Envelope Encoding → Response}[link:../../../docs/wire-codec.md]).
    #
    # 2-element msgpack array: +[status, value-or-fault]+. +status+ is 0
    # (success) or 1 (fault). For success the second element is the return
    # value; for fault it is a {Fault} (ext 0x02 envelope).
    #
    # Built on the +class X < Data.define(...)+ subclass form so the
    # class body is fully Steep-visible; see +lib/kobako/outcome/panic.rb+
    # for the rationale.
    class Response < Data.define(:status, :payload)
      def self.ok(value)
        new(status: STATUS_OK, payload: value)
      end

      def self.error(fault)
        unless fault.is_a?(Kobako::Transport::Fault)
          raise ArgumentError, "Response.error requires Kobako::Transport::Fault, got #{fault.class}"
        end

        new(status: STATUS_ERROR, payload: fault)
      end

      def initialize(status:, payload:)
        unless [STATUS_OK, STATUS_ERROR].include?(status)
          raise ArgumentError, "Response status must be 0 (ok) or 1 (error), got #{status.inspect}"
        end
        if status == STATUS_ERROR && !payload.is_a?(Kobako::Transport::Fault)
          raise ArgumentError, "Response with error status must carry a Kobako::Transport::Fault payload"
        end

        super
      end

      def ok?    = status == STATUS_OK
      def error? = status == STATUS_ERROR
    end

    def self.encode_response(response)
      Codec::Encoder.encode([response.status, response.payload])
    end

    def self.decode_response(bytes)
      arr = Codec::Decoder.decode(bytes)
      unless arr.is_a?(Array) && arr.length == 2
        raise Codec::InvalidType, "Response envelope is malformed (expected a 2-element array)"
      end

      status, payload = arr
      Codec::Utils.wire_boundary { Response.new(status: status, payload: payload) }
    end
  end
end
