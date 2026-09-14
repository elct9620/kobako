# frozen_string_literal: true

module Kobako
  module Transport
    # The methods a host object lets the guest call through one reference
    # to it — a bound path or a Capability Handle. Every Catalog entry holds
    # one, so a dispatch authorizes against the reference the guest named
    # while still running the call on, and restoring, the very object the
    # Host App handed over.
    #
    # Built on the +class X < Data.define(...)+ subclass form so the class
    # body is fully Steep-visible; see +.rubocop.yml+ for the rationale.
    class Exposure < Data.define(:object)
      # Whether the guest may call +name+ on #object. An object narrows its
      # own surface with a private +respond_to_guest?(name)+, consulted with
      # the private surface included so the guest's +public_send+ dispatch
      # can never reach the predicate itself; an object without one exposes
      # every name, leaving the reflection floor as the only refusal.
      def exposes?(name)
        return true unless object.respond_to?(:respond_to_guest?, true)

        object.__send__(:respond_to_guest?, name) ? true : false
      end
    end
  end
end
