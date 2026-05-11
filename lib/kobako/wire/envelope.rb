# frozen_string_literal: true

require_relative "error"
require_relative "handle"
require_relative "exception"
require_relative "encoder"
require_relative "decoder"

module Kobako
  module Wire
    # Envelope-layer encoders/decoders for the kobako wire contract.
    #
    # SPEC.md → Wire Contract pins the logical shape of every host↔guest
    # message and SPEC.md → Wire Codec → Envelope Frame Layout pins the
    # binary framing. This module assembles the four envelope kinds
    # (Request, Response, Result, Panic) and the outer Outcome wrapper on
    # top of the lower-level {Encoder} / {Decoder} primitives.
    #
    # The envelope objects are plain value objects; they hold the logical
    # fields and validate basic shape invariants. The actual byte layout
    # (msgpack array vs map, field ordering, outcome-tag bytes) is owned
    # by the +Envelope+ module's class methods so the Encoder/Decoder
    # primitives stay byte-only and SPEC's framing rules live in one place.
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

      # =================================================================
      # Value objects
      # =================================================================

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

      # Response envelope (SPEC.md Wire Codec → Response).
      #
      # 2-element msgpack array: [status, value-or-error]. +status+ is 0
      # (success) or 1 (error). For success the second element is the
      # return value; for error it is an {Exception} (ext 0x02 envelope).
      #
      # The two factory methods (+ok+, +err+) reflect the two mutually
      # exclusive variants pinned by SPEC. Frozen value object backed by
      # +Data.define+. Equality, +eql?+, and +hash+ are provided
      # automatically based on field values.
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

        def ok?
          status == STATUS_OK
        end

        def err?
          status == STATUS_ERROR
        end
      end

      # Result envelope (SPEC.md Outcome Envelope → Result).
      #
      # The successful Outcome payload. Wraps the deserialized last
      # expression of the mruby script. SPEC pins the Result envelope as
      # a 1-element msgpack array carrying the value, so that the framing
      # is symmetric with the Panic envelope and the value position is
      # never ambiguous.
      Result = Data.define(:value)

      # Panic envelope (SPEC.md Outcome Envelope → Panic).
      #
      # The failure Outcome payload. Encoded as a msgpack **map** keyed
      # by name (forward-compatibility — unknown keys are silently
      # ignored). Required keys: "origin", "class", "message". Optional:
      # "backtrace" (array of str), "details" (any wire-legal value).
      #
      # Frozen value object backed by +Data.define+. Equality, +eql?+, and
      # +hash+ are provided automatically based on field values.
      Panic = Data.define(:origin, :klass, :message, :backtrace, :details) do
        def initialize(origin:, klass:, message:, backtrace: [], details: nil)
          raise ArgumentError, "Panic origin must be String"  unless origin.is_a?(String)
          raise ArgumentError, "Panic class must be String"   unless klass.is_a?(String)
          raise ArgumentError, "Panic message must be String" unless message.is_a?(String)
          raise ArgumentError, "Panic backtrace must be Array" unless backtrace.is_a?(Array)

          super
        end
      end

      Panic::ORIGIN_SANDBOX = "sandbox"
      Panic::ORIGIN_SERVICE = "service"

      # Outcome envelope (SPEC.md Outcome Envelope).
      #
      # The OUTCOME_BUFFER wrapper: a one-byte tag (+0x01+ result, +0x02+
      # panic) followed by the msgpack payload. Carries either a {Result}
      # or a {Panic}.
      class Outcome
        attr_reader :payload

        def self.result(value)
          new(Result.new(value))
        end

        def self.panic(panic)
          raise ArgumentError, "Outcome.panic requires Panic" unless panic.is_a?(Panic)

          new(panic)
        end

        def initialize(payload)
          unless payload.is_a?(Result) || payload.is_a?(Panic)
            raise ArgumentError, "Outcome payload must be Result or Panic, got #{payload.class}"
          end

          @payload = payload
        end

        def result?
          @payload.is_a?(Result)
        end

        def panic?
          @payload.is_a?(Panic)
        end

        def ==(other)
          other.is_a?(Outcome) && other.payload == @payload
        end
        alias eql? ==

        def hash
          [self.class, @payload].hash
        end
      end

      # =================================================================
      # Encode / decode entry points
      # =================================================================

      # ---------------- Request ----------------

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

      # ---------------- Response ----------------

      def self.encode_response(response)
        raise ArgumentError, "encode_response requires Response" unless response.is_a?(Response)

        Encoder.encode([response.status, response.payload])
      end

      def self.decode_response(bytes)
        arr = Decoder.decode(bytes)
        unless arr.is_a?(Array) && arr.length == 2
          raise InvalidType, "Response must be a 2-element array, got #{arr.inspect}"
        end

        decode_response_status(*arr)
      end

      def self.decode_response_status(status, payload)
        case status
        when STATUS_OK
          Response.new(status: STATUS_OK, payload: payload)
        when STATUS_ERROR
          raise InvalidType, "Response status=1 payload must be ext 0x02 Exception" unless payload.is_a?(Exception)

          Response.new(status: STATUS_ERROR, payload: payload)
        else
          raise InvalidType, "Response status must be 0 or 1, got #{status.inspect}"
        end
      end
      private_class_method :decode_response_status

      # ---------------- Result envelope (Outcome payload) -----------

      def self.encode_result(value)
        Encoder.encode([value])
      end

      def self.decode_result(bytes)
        arr = Decoder.decode(bytes)
        unless arr.is_a?(Array) && arr.length == 1
          raise InvalidType, "Result envelope must be a 1-element array, got #{arr.inspect}"
        end

        Result.new(arr[0])
      end

      # ---------------- Panic envelope (Outcome payload) ------------

      def self.encode_panic(panic)
        raise ArgumentError, "encode_panic requires Panic" unless panic.is_a?(Panic)

        buf = String.new(encoding: Encoding::ASCII_8BIT)
        Encoder.new(buf).write_map_pairs(panic_pairs(panic))
        buf
      end

      # SPEC: Panic is a msgpack MAP keyed by name. We always emit the
      # required keys; "backtrace" is emitted only when non-empty (keep
      # the wire compact); "details" only when non-nil. Receivers must
      # ignore unknown keys, so the optional-key absence is wire-legal.
      def self.panic_pairs(panic)
        pairs = [
          ["origin", panic.origin],
          ["class", panic.klass],
          ["message", panic.message]
        ]
        pairs << ["backtrace", panic.backtrace] unless panic.backtrace.empty?
        pairs << ["details", panic.details] unless panic.details.nil?
        pairs
      end
      private_class_method :panic_pairs

      def self.decode_panic(bytes)
        map = Decoder.decode(bytes)
        raise InvalidType, "Panic envelope must be a map, got #{map.class}" unless map.is_a?(Hash)

        origin, klass, message = validate_panic_required_fields!(map)
        backtrace = map["backtrace"] || []
        unless backtrace.is_a?(Array) && backtrace.all?(String)
          raise InvalidType, "Panic backtrace must be array of str"
        end

        Panic.new(origin: origin, klass: klass, message: message,
                  backtrace: backtrace, details: map["details"])
      end

      def self.validate_panic_required_fields!(map)
        origin  = map["origin"]
        klass   = map["class"]
        message = map["message"]
        raise InvalidType, "Panic envelope missing 'origin' (str)"  unless origin.is_a?(String)
        raise InvalidType, "Panic envelope missing 'class' (str)"   unless klass.is_a?(String)
        raise InvalidType, "Panic envelope missing 'message' (str)" unless message.is_a?(String)

        [origin, klass, message]
      end
      private_class_method :validate_panic_required_fields!

      # ---------------- Outcome envelope (tagged wrapper) ------------

      def self.encode_outcome(outcome)
        raise ArgumentError, "encode_outcome requires Outcome" unless outcome.is_a?(Outcome)

        tag, body = encode_outcome_payload(outcome.payload)
        out = String.new(encoding: Encoding::ASCII_8BIT)
        out << [tag].pack("C")
        out << body
        out
      end

      def self.encode_outcome_payload(payload)
        case payload
        when Result then [OUTCOME_TAG_RESULT, encode_result(payload.value)]
        when Panic  then [OUTCOME_TAG_PANIC, encode_panic(payload)]
        end
      end
      private_class_method :encode_outcome_payload

      def self.decode_outcome(bytes)
        bytes = bytes.b
        raise InvalidType, "Outcome bytes must not be empty" if bytes.empty?

        tag = bytes.getbyte(0)
        body = bytes.byteslice(1, bytes.bytesize - 1)
        Outcome.new(decode_outcome_payload(tag, body))
      end

      def self.decode_outcome_payload(tag, body)
        case tag
        when OUTCOME_TAG_RESULT then decode_result(body)
        when OUTCOME_TAG_PANIC  then decode_panic(body)
        else raise InvalidType, format("unknown outcome tag 0x%<tag>02x", tag: tag)
        end
      end
      private_class_method :decode_outcome_payload

      # =================================================================
      # Internal helpers
      # =================================================================

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
