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
    # idiom routes to) with +provider+ (the source of the bound object).
    #
    # A +provider+ that is not itself callable is the bound object, resolved
    # once for the Sandbox's life; a callable provider is invoked once per
    # invocation to yield that invocation's object, so a fresh object backs
    # the path every invocation. Callability is the sole discriminator — a
    # fixed backend that is itself callable is supplied through a
    # non-callable wrapper.
    class Backend < Data.define(:path, :provider)
    end

    # +backend+ and +depends_on+ default to absent so the common
    # pure-idiom and single-backend shapes stay terse.
    def initialize(name:, source:, backend: nil, depends_on: [])
      super
    end
  end
end
