# frozen_string_literal: true

require_relative "../codec"
require_relative "../fault"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns the Response value object and its +#encode+ / +.decode+ codec,
  # plus the +STATUS_OK+ / +STATUS_ERROR+ status bytes.
  module Transport
    # ---------------- Response status bytes (docs/wire-contract.md § Response Shape) ---

    # Response variant marker for the success branch.
    STATUS_OK    = 0
    # Response variant marker for the fault branch.
    STATUS_ERROR = 1

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
        unless fault.is_a?(Kobako::Fault)
          raise ArgumentError, "Response.error requires Kobako::Fault, got #{fault.class}"
        end

        new(status: STATUS_ERROR, payload: fault)
      end

      # Decode +bytes+ into a {Response}. Raises +Codec::InvalidType+ when the
      # envelope is not the expected 2-element msgpack array, or when the
      # Value Object's construction invariants reject the decoded fields.
      def self.decode(bytes)
        Codec::Decoder.decode(bytes) do |arr|
          unless arr.is_a?(Array) && arr.length == 2
            raise Codec::InvalidType, "Response envelope is malformed (expected a 2-element array)"
          end

          status, payload = arr
          new(status: status, payload: payload)
        end
      end

      def initialize(status:, payload:)
        unless [STATUS_OK, STATUS_ERROR].include?(status)
          raise ArgumentError, "Response status must be 0 (ok) or 1 (error), got #{status.inspect}"
        end
        if status == STATUS_ERROR && !payload.is_a?(Kobako::Fault)
          raise ArgumentError, "Response with error status must carry a Kobako::Fault payload"
        end

        super
      end

      def ok?    = status == STATUS_OK
      def error? = status == STATUS_ERROR

      # Encode this Response to msgpack bytes as the 2-element
      # +[status, payload]+ array.
      def encode
        Codec::Encoder.encode([status, payload])
      end
    end
  end
end
