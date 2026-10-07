# frozen_string_literal: true

require_relative "../codec"
require_relative "../errors"
require_relative "../payload"
require_relative "call"
require_relative "reflection"
require_relative "yielder"

module Kobako
  # See lib/kobako/transport.rb for the umbrella module doc; this file
  # owns the pure-function dispatcher that answers a routed Call.
  module Transport
    # Answers a routed Call with +[ok, bytes]+ and never raises, so every
    # failure reaches the guest as a fault.
    module Dispatcher # :nodoc:
      # Throw tag for the Yielder's break unwind back to the
      # dispatcher's +catch+ frame. +private_constant+ is a
      # convention boundary — not a defence.
      BREAK_THROW = :__kobako_break__
      private_constant :BREAK_THROW

      class UndefinedTargetError < StandardError; end # :nodoc:

      class UnreadableRequestError < Kobako::Codec::Error; end # :nodoc:

      # The category kobako's own refusals answer under, keyed by the class
      # each is raised as and ordered most specific first. A class absent
      # here is the Service's own exception, which answers under +runtime+
      # wearing the +<class>: <message>+ shape that says so.
      #
      # +Kobako::Codec::Error+ is the floor rather than a path of its own:
      # a codec fault reaching the boundary unnamed is the exchange failing,
      # and must not be dressed as something a Service raised.
      OWN_FAULTS = {
        HandleExhaustedError => "internal",
        UndefinedTargetError => "undefined",
        ArgumentError => "argument",
        YieldValueError => "runtime",
        Kobako::Codec::Error => "internal",
        Kobako::SandboxError => "runtime"
      }.freeze
      private_constant :OWN_FAULTS

      # The per-invocation state arrives as arguments so the Dispatcher
      # stays stateless and neither the resolver nor the Context publishes
      # accessors for it.
      def self.dispatch(call, resolver, handles, yield_to_guest)
        yielder = Yielder.new(yield_to_guest, BREAK_THROW, handles) if call.block_given
        [true, encode_ok(run(call, resolver, handles, yielder), handles), nil] # : [bool, String, String?]
      # StandardError is the boundary by intent: a Service method's
      # application fault folds into a guest-rescuable fault, while a
      # host-process failure (NoMemoryError, SignalException, a bare Exception)
      # stays uncaught and traps the invocation rather than being masked as a
      # rescuable fault.
      rescue StandardError => e
        [false, *caught_fault(e, yielder)] # : [bool, String, String?]
      ensure
        yielder&.invalidate!
      end

      class << self
        private

        def run(call, resolver, handles, yielder)
          arguments, carried_handle = decode_arguments(call.payload)
          exposure = resolve_target(call.target, resolver, handles)
          args, kwargs = resolve_call_args(arguments, handles, carried_handle)
          catch(BREAK_THROW) { invoke(exposure, call.method_name, args, kwargs, yielder) }
        end

        # A codec fault here is a request that never became a call, restated
        # so it cannot read as an unwritable reply.
        def decode_arguments(payload)
          Kobako::Codec.track_handles { Payload::Arguments.decode(payload) }
        rescue Kobako::Codec::Error => e
          raise UnreadableRequestError, "Sandbox could not read the request: #{e.message}"
        end

        def resolve_call_args(arguments, handles, carried_handle)
          return [arguments.args, arguments.kwargs] unless carried_handle

          [arguments.args.map { |v| resolve_arg(v, handles) },
           arguments.kwargs.transform_values { |v| resolve_arg(v, handles) }]
        end

        # The class prefix marks a Service's own exception and nothing else:
        # it is the +<class>: <message>+ shape a Host App is told to keep
        # secrets out of, so wearing it says the Service raised. kobako's own
        # refusals answer under their own wording instead of borrowing that
        # shape.
        #
        # The guest's own block failing is not the Service's to report at
        # all, so the Yielder that raised it is asked first — it recognises
        # its own by identity and words the failure the guest's way.
        def caught_fault(error, yielder)
          block_failure = yielder&.fault_text(error)
          return fault("block", block_failure) if block_failure

          own = OWN_FAULTS.find { |klass, _| error.is_a?(klass) }
          return fault(own.last, error.message) if own

          fault("runtime", "#{error.class}: #{error.message}")
        end

        # The wire always carries a keyword map, so an empty one is left
        # unsplatted for methods that take no keywords.
        def invoke(exposure, method, args, kwargs, yielder = nil)
          name = method.to_sym
          reject_unreachable!(exposure, name)
          target = exposure.object
          block = yielder&.to_proc
          if kwargs.empty?
            target.public_send(name, *args, &block)
          else
            target.public_send(name, *args, **kwargs, &block)
          end
        end

        # Both the ambient-surface floor and the reference's Exposure answer
        # through Reflection, so a rejected name discloses nothing about
        # which of the two refused.
        def reject_unreachable!(exposure, name)
          reason = Reflection.refusal(exposure, name)
          raise UndefinedTargetError, reason if reason
        end

        def resolve_arg(value, handles)
          Kobako::Codec::HandleWalk.deep_restore(value, handles)
        rescue Kobako::SandboxError => e
          raise UndefinedTargetError, e.message
        end

        # The envelope already discriminated the two target forms, so no
        # else-branch is needed.
        def resolve_target(target, resolver, handles)
          case target
          when String
            resolve_path(target, resolver)
          when Integer
            resolve_handle(target, handles)
          end
        end

        def resolve_path(path, resolver)
          resolver.lookup(path)
        rescue KeyError => e
          raise UndefinedTargetError, e.message
        end

        def resolve_handle(id, handles)
          handles.exposure(id)
        rescue Kobako::SandboxError => e
          raise UndefinedTargetError, e.message
        end

        # A value with no wire form goes back as a Capability Handle. Any
        # other codec fault is the answer failing to encode, which only the
        # Service can change — so it is named here, where the direction is
        # known, instead of falling to the boundary's codec floor.
        def encode_ok(value, handles)
          Kobako::Codec::Nesting.assert_within_bound!(value)
          Kobako::Codec::Encoder.encode(value)
        rescue Kobako::Codec::UnsupportedTypeError
          encode_ok(wrap_as_handle(value, handles), handles)
        rescue Kobako::Codec::Error => e
          raise Kobako::SandboxError, "Sandbox could not write the Service's answer: #{e.message}"
        end

        def wrap_as_handle(value, handles)
          handles.alloc(value)
        end

        # Ruby core builds some exception messages as ASCII-8BIT (the arity
        # ArgumentError, for one), and the envelope requires UTF-8 of the
        # text fields it frames.
        def fault(type, message)
          [message.encode(Encoding::UTF_8, invalid: :replace, undef: :replace), type] # : [String, String]
        end
      end
    end
  end
end
