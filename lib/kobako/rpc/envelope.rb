# frozen_string_literal: true

require_relative "handle"
require_relative "fault"
require_relative "../codec"

module Kobako
  # See lib/kobako/rpc.rb for the umbrella module doc; this file owns the
  # Request / Response value objects and their encode/decode helpers.
  module RPC
    # ---------------- Response status bytes (SPEC.md Response Shape) ---

    # Response variant marker for the success branch.
    STATUS_OK    = 0
    # Response variant marker for the fault branch.
    STATUS_ERROR = 1

    # Value object for a single guest-initiated RPC Request
    # ({SPEC.md Wire Codec → Request}[link:../../../SPEC.md]).
    #
    # 4-element msgpack array: +[target, method, args, kwargs]+. +target+
    # is either a +String+ (+"Namespace::Member"+) or a {Handle}. SPEC pins
    # +kwargs+ map keys to ext 0x00 Symbol; enforced at construction so the
    # Value Object is the single source of truth.
    Request = Data.define(:target, :method_name, :args, :kwargs) do
      # steep:ignore:start
      def initialize(target:, method:, args: [], kwargs: {})
        unless target.is_a?(String) || target.is_a?(Kobako::RPC::Handle)
          raise ArgumentError, "Request target must be String or Kobako::RPC::Handle, got #{target.class}"
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
      # steep:ignore:end
    end

    # Encode a {Request} to msgpack bytes. The Value Object's own
    # invariants are the contract; this method does not re-check the shape.
    def self.encode_request(request)
      Codec::Encoder.encode([request.target, request.method_name, request.args, request.kwargs])
    end

    def self.decode_request(bytes)
      arr = Codec::Decoder.decode(bytes)
      unless arr.is_a?(Array) && arr.length == 4
        raise Codec::InvalidType, "Request must be a 4-element array, got #{arr.inspect}"
      end

      target, method_name, args, kwargs = arr
      Codec::Utils.translate_value_object_error do
        Request.new(target: target, method: method_name, args: args, kwargs: kwargs)
      end
    end

    # Value object for a single host-side RPC Response
    # ({SPEC.md Wire Codec → Response}[link:../../../SPEC.md]).
    #
    # 2-element msgpack array: +[status, value-or-fault]+. +status+ is 0
    # (success) or 1 (fault). For success the second element is the return
    # value; for fault it is a {Fault} (ext 0x02 envelope).
    Response = Data.define(:status, :payload) do
      # steep:ignore:start
      def self.ok(value)
        new(status: STATUS_OK, payload: value)
      end

      def self.err(fault)
        unless fault.is_a?(Kobako::RPC::Fault)
          raise ArgumentError, "Response.err requires Kobako::RPC::Fault, got #{fault.class}"
        end

        new(status: STATUS_ERROR, payload: fault)
      end

      def initialize(status:, payload:)
        unless [STATUS_OK, STATUS_ERROR].include?(status)
          raise ArgumentError, "Response status must be 0 or 1, got #{status.inspect}"
        end
        if status == STATUS_ERROR && !payload.is_a?(Kobako::RPC::Fault)
          raise ArgumentError, "Response status=1 payload must be Kobako::RPC::Fault"
        end

        super
      end

      def ok?  = status == STATUS_OK
      def err? = status == STATUS_ERROR
      # steep:ignore:end
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
      Codec::Utils.translate_value_object_error { Response.new(status: status, payload: payload) }
    end
  end
end
