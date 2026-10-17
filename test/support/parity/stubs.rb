# frozen_string_literal: true

module Parity
  # Stub receivers built from a scenario method-set, shared by bound
  # Services, Extension backends, and a per-invocation +ctx.bind+
  # override. The Rust runner's StubReceiver interprets the same tags.
  module Stubs
    # The closed stub-behavior set, one builder per tag.
    # +echo_positional+ takes no keyword arguments on purpose — kwargs
    # on the wire must fail its parameter binding; +yield_each+ yields
    # each positional argument and returns the array of block results;
    # +opaque+ hands back the same labeled non-wire object on every
    # call so identity is observable, and +read_label+ reads it off a
    # (possibly Array-nested) restored argument.
    BODIES = {
      "echo" => ->(_behavior) { ->(arg = nil, *, **) { arg } },
      "echo_positional" => ->(_behavior) { ->(arg = nil) { arg } },
      "value" => lambda { |behavior|
        constant = ValueTags.untag(behavior.fetch(:value))
        ->(*, **) { constant }
      },
      "raise" => lambda { |behavior|
        message = behavior.fetch(:message, "stub failure")
        ->(*, **) { raise message }
      },
      "yield_each" => ->(_behavior) { ->(*args, **, &blk) { args.map { |arg| blk.call(arg) } } },
      "opaque" => lambda { |behavior|
        constant = OpaqueObject.new(behavior.fetch(:label))
        ->(*, **) { constant }
      },
      "read_label" => lambda { |_behavior|
        lambda { |arg, *, **|
          arg = arg.first while arg.is_a?(Array)
          arg.label
        }
      },
      "counter" => lambda { |_behavior|
        count = 0
        ->(*, **) { count += 1 }
      }
    }.freeze

    def self.build(methods, exposed = nil)
      stub = Object.new
      (methods || {}).each do |name, behavior|
        stub.define_singleton_method(name, &body(behavior))
      end
      narrow_guest_surface(stub, exposed) if exposed
      stub
    end

    # A service's optional +exposed+ list is the scenario's
    # +respond_to_guest?+ narrowing: the predicate is defined private,
    # so the guest can never dispatch to it.
    def self.narrow_guest_surface(stub, exposed)
      allowed = exposed.map(&:to_s)
      stub.define_singleton_method(:respond_to_guest?) { |name| allowed.include?(name.to_s) }
      stub.singleton_class.class_eval { private :respond_to_guest? }
    end

    def self.body(behavior)
      builder = BODIES.fetch(behavior.fetch(:behavior)) do
        raise ArgumentError, "unknown stub behavior: #{behavior.inspect}"
      end
      builder.call(behavior)
    end

    private_class_method :narrow_guest_surface, :body
  end
end
