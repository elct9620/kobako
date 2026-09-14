# frozen_string_literal: true

require "delegate"

require_relative "reflection"

module Kobako
  module Transport
    # The methods a host object lets the guest call through one reference
    # to it — a bound path or a Capability Handle — fixed when the reference
    # is made. An object defining its own private +respond_to_guest?(name)+
    # is asked on every dispatch; any other exposes only what its own class
    # and the object itself define in source, since the methods a Host App
    # cannot foresee handing over are the ones it never wrote.
    #
    # Built on the +class X < Data.define(...)+ subclass form so the class
    # body is fully Steep-visible; see +.rubocop.yml+ for the rationale.
    class Exposure < Data.define(:object, :names)
      PREDICATE = :respond_to_guest?
      private_constant :PREDICATE

      # Kernel's own reflection, bound onto each object so a forwarder or a
      # BasicObject answers from its own method table rather than through
      # +method_missing+ / +respond_to_missing?+.
      CLASS_OF = Kernel.instance_method(:class)
      SINGLETON_CLASS_OF = Kernel.instance_method(:singleton_class)
      SINGLETON_METHODS = Kernel.instance_method(:singleton_methods)
      PRIVATE_METHODS = Kernel.instance_method(:private_methods)
      private_constant :CLASS_OF, :SINGLETON_CLASS_OF, :SINGLETON_METHODS, :PRIVATE_METHODS

      CALLABLE = Set.new(Reflection::CALLABLE_ALLOW).freeze
      NOTHING = Set.new.freeze
      private_constant :CALLABLE, :NOTHING

      # The Exposure +object+ carries from this moment on. +surfaces+ caches
      # each class's own surface for a caller that makes many references at
      # once (a Handle table), so two objects of one class enumerate it once.
      def self.of(object, surfaces = {})
        klass = CLASS_OF.bind_call(object)
        names = narrows_itself?(object, klass) ? nil : default_names(object, klass, surfaces)
        new(object: object, names: names)
      end

      # Whether the guest may call +name+ through this reference. The
      # object's own predicate is consulted with the private surface
      # included, so the guest's +public_send+ can never reach it.
      def exposes?(name)
        return names.include?(name) if names

        object.__send__(PREDICATE, name) ? true : false
      end

      class << self
        private

        # Whether +object+ defines the narrowing predicate itself — read off
        # the method tables, so a forwarder's target or a catch-all
        # +respond_to_missing?+ cannot answer for it.
        def narrows_itself?(object, klass)
          klass.method_defined?(PREDICATE) || klass.private_method_defined?(PREDICATE) ||
            PRIVATE_METHODS.bind_call(object, false).include?(PREDICATE) ||
            SINGLETON_METHODS.bind_call(object, false).include?(PREDICATE)
        end

        # A callable keeps its callable names; a class-level surface or a
        # forwarder has no surface of its own to expose.
        def default_names(object, klass, surfaces)
          case object
          when Proc, Method then CALLABLE
          when Module, Delegator then NOTHING
          else with_singleton(object, surfaces[klass] ||= class_names(object, klass))
          end
        end

        def class_names(object, klass)
          names = authored(klass)
          case object
          when Struct, Data then names.concat(object.members)
          end
          Set.new(names).freeze
        end

        def with_singleton(object, names)
          return names if SINGLETON_METHODS.bind_call(object, false).empty?

          names.union(authored(SINGLETON_CLASS_OF.bind_call(object))).freeze
        end

        # The public methods +owner+ itself defines in source. A method with
        # no source, or whose source is the platform's own, is built in.
        def authored(owner)
          owner.public_instance_methods(false).select do |name|
            file, = owner.instance_method(name).source_location
            file && !file.start_with?("<internal:")
          end
        end
      end
    end
  end
end
