# frozen_string_literal: true

require_relative "../handle"
require_relative "../codec"

module Kobako
  module Wire
    # Envelope-layer value objects and encode/decode helpers. See envelope.rb.
    module Envelope
      # Request envelope (SPEC.md Wire Codec → Request).
      #
      # 4-element msgpack array: [target, method, args, kwargs].
      # +target+ is either a String (e.g. "Group::Member") or a {Handle}.
      # +method+ is a String. +args+ is an Array. +kwargs+ is a Hash with
      # String keys.
      #
      # Frozen value object backed by +Data.define+. Equality, +eql?+, and
      # +hash+ are provided automatically based on field values. The public
      # constructor keyword is +method:+ (not +method_name:+); the
      # +initialize+ override maps it to the Data field +method_name:+.
      # SPEC.md Wire Codec → str/bin Encoding Rules: Request +kwargs+ keys
      # must be UTF-8. We enforce String-typed keys at construction so the
      # Value Object is the single source of this invariant; +decode_request+
      # translates the resulting ArgumentError into a wire-layer +InvalidType+.
      Request = Data.define(:target, :method_name, :args, :kwargs) do
        def initialize(target:, method:, args: [], kwargs: {})
          Envelope.send(:validate_request_fields!, target, method, args, kwargs)
          super(target: target, method_name: method, args: args, kwargs: kwargs)
        end
      end

      def self.validate_request_fields!(target, method_name, args, kwargs)
        unless target.is_a?(String) || target.is_a?(Handle)
          raise ArgumentError, "Request target must be String or Handle, got #{target.class}"
        end
        raise ArgumentError, "Request method must be String" unless method_name.is_a?(String)
        raise ArgumentError, "Request args must be Array"    unless args.is_a?(Array)

        validate_request_kwargs!(kwargs)
      end
      private_class_method :validate_request_fields!

      def self.validate_request_kwargs!(kwargs)
        raise ArgumentError, "Request kwargs must be Hash" unless kwargs.is_a?(Hash)

        kwargs.each_key do |k|
          raise ArgumentError, "Request kwargs keys must be String, got #{k.class}" unless k.is_a?(String)
        end
      end
      private_class_method :validate_request_kwargs!

      # ---------------- Request encode / decode ----------------

      # Encode a {Request} (or its three constituent fields) to bytes.
      def self.encode_request(target_or_request, method_name = nil, args = nil, kwargs = nil)
        req = if target_or_request.is_a?(Request)
                target_or_request
              else
                Request.new(target: target_or_request, method: method_name, args: args || [],
                            kwargs: kwargs || {})
              end

        Encoder.encode([req.target, req.method_name, req.args, req.kwargs])
      end

      # Decode bytes to a {Request}.
      def self.decode_request(bytes)
        arr = Decoder.decode(bytes)
        unless arr.is_a?(Array) && arr.length == 4
          raise InvalidType, "Request must be a 4-element array, got #{arr.inspect}"
        end

        target, method_name, args, kwargs = arr
        Request.new(target: target, method: method_name, args: args, kwargs: kwargs)
      rescue ArgumentError => e
        raise InvalidType, e.message
      end
    end
  end
end
