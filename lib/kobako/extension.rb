# frozen_string_literal: true

module Kobako
  # Kobako::Extension — a guest idiom paired with an optional host backend,
  # installed on a Sandbox via +Sandbox#install+. It composes the existing
  # +#preload+ (the guest +source+) and +#bind+ (the +backend+) verbs into
  # one setup unit, so guest code sees a native-style constant whose pure
  # methods run in-guest and whose privileged methods dispatch to the
  # backend.
  #
  # The four readers form the contract +#install+ duck-types on:
  #
  #   * +name+ — a Symbol matching +/\A[A-Z]\w*\z/+, the preloaded snippet's
  #     canonical backtrace name and the +depends_on+ match key. Independent
  #     of any bound path.
  #   * +source+ — the mruby idiom as a String; always present, since an
  #     Extension always carries a guest idiom. A host object with no idiom
  #     is bound with +#bind+ directly.
  #   * +backend+ — an +Extension::Backend+ or +nil+ for a pure-guest
  #     Extension.
  #   * +depends_on+ — Symbol names of Extensions that must also be
  #     installed, checked for presence at the first invocation.
  #
  # +Kobako::Extension+ is the bundled value type; any object exposing the
  # four readers is equally valid, so a Host App or gem may supply its own.
  class Extension < Data.define(:name, :source, :backend, :depends_on)
    # Kobako::Extension::Backend — the host attachment of an Extension,
    # pairing +path+ (the constant path the backend binds at, single-segment
    # +"File"+ or nested +"MyApp::Store"+, spelling the guest constant the
    # idiom routes to) with the source of the bound object, declared by
    # explicit keyword:
    #
    #   * +object:+ — a static object, fixed for the Sandbox's life.
    #   * +provider:+ — a no-argument callable invoked once per invocation to
    #     yield that invocation's object, so a fresh object backs the path
    #     every invocation. A provider that raises propagates its exception to
    #     the invocation caller and leaves the guest unrun; the next
    #     invocation resolves it afresh.
    #   * neither — a fillable, defaulting to +Kobako::Unresolved+ until the
    #     host supplies the invocation's object.
    #
    # The kind is chosen by keyword, never inferred from whether the value is
    # callable, so a static object that is itself callable is expressed
    # directly with +object:+. Giving both +object:+ and +provider:+ raises
    # +ArgumentError+.
    class Backend < Data.define(:path, :object, :provider)
      def initialize(path:, object: nil, provider: nil)
        if !object.nil? && !provider.nil?
          raise ArgumentError,
                "Extension::Backend accepts object: or provider:, not both"
        end

        super
      end
    end

    # +backend+ and +depends_on+ default to absent so the common
    # pure-idiom and single-backend shapes stay terse.
    def initialize(name:, source:, backend: nil, depends_on: [])
      super
    end
  end
end
