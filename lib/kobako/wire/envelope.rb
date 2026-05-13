# frozen_string_literal: true

require_relative "handle"
require_relative "exception"
require_relative "codec"

module Kobako
  module Wire
    # Envelope-layer encoders/decoders for the kobako wire contract.
    #
    # SPEC.md → Wire Contract pins the logical shape of every host↔guest
    # message and SPEC.md → Wire Codec → Envelope Frame Layout pins the
    # binary framing. This module assembles the four envelope kinds
    # (Request, Response, Result, Panic) and the outer Outcome wrapper on
    # top of the lower-level {Codec::Encoder} / {Codec::Decoder} primitives.
    #
    # The contract collapses into two wire paths:
    #
    #   - **RPC path** (lives in this file): Request / Response — guest
    #     calls a Service, host returns a value or an Exception.
    #   - **Outcome path** (lives in +envelope/payloads.rb+): Result /
    #     Panic wrapped in an Outcome envelope — the host reads this
    #     after +__kobako_run+ to surface either the script's last
    #     expression or a Sandbox/Service panic.
    #
    # The envelope objects are plain Value Objects; they own the field
    # invariants (raising +ArgumentError+ on violation). The encode/decode
    # helpers around them own the msgpack framing and translate value-
    # object faults into the wire-layer +Codec::InvalidType+ taxonomy.
    module Envelope
      # ---------------- Outcome tag bytes (SPEC.md Outcome Envelope) -----

      # First byte of the OUTCOME_BUFFER for a Result envelope.
      OUTCOME_TAG_RESULT = 0x01
      # First byte of the OUTCOME_BUFFER for a Panic envelope.
      OUTCOME_TAG_PANIC  = 0x02

      # ---------------- Response status bytes (SPEC.md Response Shape) ---

      # Response variant marker for the success branch.
      STATUS_OK    = 0
      # Response variant marker for the error branch.
      STATUS_ERROR = 1

      # ============================================================
      # Request (SPEC.md Wire Codec → Request)
      # ============================================================
      #
      # 4-element msgpack array: [target, method, args, kwargs]. +target+
      # is either a String ("Group::Member") or a {Handle}. SPEC pins
      # +kwargs+ map keys to ext 0x00 Symbol (→ Wire Codec → Ext Types);
      # enforced at construction so the Value Object is the single source
      # of truth.
      Request = Data.define(:target, :method_name, :args, :kwargs) do
        def initialize(target:, method:, args: [], kwargs: {})
          unless target.is_a?(String) || target.is_a?(Handle)
            raise ArgumentError, "Request target must be String or Handle, got #{target.class}"
          end
          raise ArgumentError, "Request method must be String" unless method.is_a?(String)
          raise ArgumentError, "Request args must be Array"    unless args.is_a?(Array)

          validate_kwargs!(kwargs)
          super(target: target, method_name: method, args: args, kwargs: kwargs)
        end

        private

        def validate_kwargs!(kwargs)
          raise ArgumentError, "Request kwargs must be Hash" unless kwargs.is_a?(Hash)

          kwargs.each_key do |k|
            raise ArgumentError, "Request kwargs keys must be Symbol, got #{k.class}" unless k.is_a?(Symbol)
          end
        end
      end

      # Encode a {Request} to bytes. The Value Object's own invariants
      # are the contract; this method does not re-check the shape.
      def self.encode_request(request)
        Codec::Encoder.encode([request.target, request.method_name, request.args, request.kwargs])
      end

      def self.decode_request(bytes)
        arr = Codec::Decoder.decode(bytes)
        unless arr.is_a?(Array) && arr.length == 4
          raise Codec::InvalidType, "Request must be a 4-element array, got #{arr.inspect}"
        end

        target, method_name, args, kwargs = arr
        Codec.translate_value_object_error do
          Request.new(target: target, method: method_name, args: args, kwargs: kwargs)
        end
      end

      # ============================================================
      # Response (SPEC.md Wire Codec → Response)
      # ============================================================
      #
      # 2-element msgpack array: [status, value-or-error]. +status+ is 0
      # (success) or 1 (error). For success the second element is the
      # return value; for error it is an {Exception} (ext 0x02 envelope).
      Response = Data.define(:status, :payload) do
        def self.ok(value)
          new(status: STATUS_OK, payload: value)
        end

        def self.err(exception)
          unless exception.is_a?(Exception)
            raise ArgumentError, "Response.err requires Kobako::Wire::Exception, got #{exception.class}"
          end

          new(status: STATUS_ERROR, payload: exception)
        end

        def initialize(status:, payload:)
          unless [STATUS_OK, STATUS_ERROR].include?(status)
            raise ArgumentError, "Response status must be 0 or 1, got #{status.inspect}"
          end
          if status == STATUS_ERROR && !payload.is_a?(Exception)
            raise ArgumentError, "Response status=1 payload must be Kobako::Wire::Exception"
          end

          super
        end

        def ok?  = status == STATUS_OK
        def err? = status == STATUS_ERROR
      end

      def self.encode_response(response)
        Codec::Encoder.encode([response.status, response.payload])
      end

      def self.decode_response(bytes)
        arr = Codec::Decoder.decode(bytes)
        unless arr.is_a?(Array) && arr.length == 2
          raise Codec::InvalidType, "Response must be a 2-element array, got #{arr.inspect}"
        end

        status, payload = arr
        Codec.translate_value_object_error { Response.new(status: status, payload: payload) }
      end
    end
  end
end

require_relative "envelope/payloads"
