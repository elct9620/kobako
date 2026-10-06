# frozen_string_literal: true

module Kobako
  # Guest source paired with an optional host backend, installed with
  # Sandbox#install: the guest sees one constant whose pure methods run in the
  # guest and whose privileged ones call the backend. Any object answering the
  # same four readers installs the same way.
  #
  # [+name+]        A constant-form Symbol: the snippet's backtrace name, and
  #                 what another Extension's +depends_on+ names.
  # [+source+]      The mruby source, a String. A host object with no source
  #                 is bound with Sandbox#bind instead.
  # [+backend+]     A Backend, or +nil+ for an Extension that runs entirely in
  #                 the guest.
  # [+depends_on+]  Names of Extensions that must also be installed, checked
  #                 at the first invocation.
  class Extension < Data.define(:name, :source, :backend, :depends_on)
    # The host object an Extension's constant at +path+ calls, such as
    # <tt>"MyApp::Store"</tt>, given by keyword:
    #
    # [+object:+]   One object for the Sandbox's life.
    # [+provider:+] A callable taking no arguments, called once per invocation
    #               for that invocation's object. If it raises, the exception
    #               reaches the caller and the guest does not run.
    # [neither]     A fillable Service, supplied per invocation through
    #               Context#bind.
    #
    # The keyword decides the kind, so a callable object meant as itself is
    # given as +object:+. Giving both raises ArgumentError.
    class Backend < Data.define(:path, :object, :provider)
      def initialize(path:, object: nil, provider: nil)
        if !object.nil? && !provider.nil?
          raise ArgumentError,
                "Extension::Backend accepts object: or provider:, not both"
        end

        super
      end
    end

    # +backend+ defaults to none, and +depends_on+ to no other Extension.
    def initialize(name:, source:, backend: nil, depends_on: [])
      super
    end
  end
end
