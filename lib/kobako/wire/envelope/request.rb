# frozen_string_literal: true

require_relative "../handle"
require_relative "../encoder"
require_relative "../decoder"
require_relative "../error"

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
      Request = Data.define(:target, :method_name, :args, :kwargs) do
        def initialize(target:, method:, args: [], kwargs: {})
          unless target.is_a?(String) || target.is_a?(Handle)
            raise ArgumentError, "Request target must be String or Handle, got #{target.class}"
          end
          raise ArgumentError, "Request method must be String" unless method.is_a?(String)
          raise ArgumentError, "Request args must be Array"    unless args.is_a?(Array)
          raise ArgumentError, "Request kwargs must be Hash"   unless kwargs.is_a?(Hash)

          super(target: target, method_name: method, args: args, kwargs: kwargs)
        end
      end

      # ---------------- Request encode / decode ----------------

      # Encode a {Request} (or its three constituent fields) to bytes.
      def self.encode_request(target_or_request, method_name = nil, args = nil, kwargs = nil)
        req = if target_or_request.is_a?(Request)
                target_or_request
              else
                Request.new(target: target_or_request, method: method_name, args: args || [],
                            kwargs: kwargs || {})
              end

        validate_kwargs_keys!(req.kwargs)
        Encoder.encode([req.target, req.method_name, req.args, req.kwargs])
      end

      # Decode bytes to a {Request}.
      def self.decode_request(bytes)
        arr = Decoder.decode(bytes)
        unless arr.is_a?(Array) && arr.length == 4
          raise InvalidType, "Request must be a 4-element array, got #{arr.inspect}"
        end

        target, method_name, args, kwargs = arr
        validate_request_fields!(target, method_name, args, kwargs)
        Request.new(target: target, method: method_name, args: args, kwargs: kwargs)
      end

      def self.validate_request_fields!(target, method_name, args, kwargs)
        unless target.is_a?(String) || target.is_a?(Handle)
          raise InvalidType, "Request target must be str or Handle, got #{target.class}"
        end
        raise InvalidType, "Request method must be str" unless method_name.is_a?(String)
        raise InvalidType, "Request args must be array" unless args.is_a?(Array)
        raise InvalidType, "Request kwargs must be map" unless kwargs.is_a?(Hash)

        validate_kwargs_keys!(kwargs)
      end
      private_class_method :validate_request_fields!

      # SPEC.md Wire Codec → str/bin Encoding Rules: Request `kwargs` keys
      # must be UTF-8 (str family or bin-with-UTF-8-validated bytes). At the
      # envelope layer we only have the high-level Hash; reject keys that
      # are not String to keep the boundary tight.
      def self.validate_kwargs_keys!(kwargs)
        kwargs.each_key do |k|
          raise InvalidType, "Request kwargs keys must be String, got #{k.class}" unless k.is_a?(String)
        end
      end
    end
  end
end
