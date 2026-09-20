# frozen_string_literal: true

require "test_helper"

# Coverage for Kobako::Sandbox#run host→guest argument auto-wrap —
# non-wire-representable args / kwargs values are routed through the
# Sandbox's Catalog::Handles and arrive in the guest as Kobako::Handle
# proxies whose method calls dispatch back as transport calls. The
# forged-Handle reject path lives in
# test/e2e/sandbox/test_run_preflight.rb alongside the rest of the #run
# host pre-flight error coverage; this file is the e2e elevation of the
# auto-wrap happy path against the real data/kobako.wasm.
class TestSandboxRunAutoWrap < Minitest::Test
  include E2eGuestHelper

  # A request body the Host App wrote: no wire representation, and a
  # +#read+ of its own for the guest to call back through.
  class Body
    def initialize(text) = @text = text
    def read = @text
  end

  # A host object arrives as a positional argument. The host wraps it as
  # a Handle; the guest receives a proxy at the same arg position and
  # +#read+ on the proxy round-trips to the host object.
  # @behavior T-066
  def test_positional_host_object_round_trips_via_handle_proxy
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Echo = ->(body) { body.read.upcase }", name: :Echo)

    assert_equal "HELLO WORLD", sandbox.run(:Echo, Body.new("hello world")).value
  end

  # Same auto-wrap path through the kwargs branch — exercises the
  # symmetric deep_wrap walk over Hash values.
  # @behavior T-067
  def test_kwargs_value_host_object_round_trips_via_handle_proxy
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "App = ->(opts) { opts[:body].read }", name: :App)

    assert_equal "payload", sandbox.run(:App, body: Body.new("payload")).value
  end

  # Auto-wrap applies to Hash values, not keys: a non-wire-representable
  # object may cross as a value (above) but not as a key. #run rejects such
  # a key with a public SandboxError rather than leaking the internal codec
  # UnsupportedTypeError that a raw encode would otherwise raise.
  # @behavior T-068
  def test_non_representable_hash_key_argument_is_rejected_as_sandbox_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "App = ->(h) { h.size }", name: :App)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.run(:App, { StringIO.new("k") => "v" })
    end

    assert_match(/key that cannot cross the sandbox boundary \(StringIO\)/, err.message,
                 "a Hash key #run cannot auto-wrap must name the key's type — the sibling " \
                 "depth refusal raises the same class, so only the wording tells them apart")
  end

  # A cyclic argument nests without bound and cannot faithfully cross. The
  # host refuses it while encoding the Run payload, so #run surfaces a
  # clean SandboxError before entering the guest rather than a host stack
  # overflow escaping the invocation.
  # @behavior T-069
  def test_cyclic_argument_is_rejected_as_sandbox_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "App = ->(x) { x }", name: :App)
    cyclic = []
    cyclic << cyclic

    err = assert_raises(Kobako::SandboxError) { sandbox.run(:App, cyclic) }

    assert_match(/nests deeper than 128 levels/, err.message,
                 "a cyclic #run argument must be refused for its depth, so the refusal " \
                 "is not mistaken for the sibling unwrappable-key one")
  end

  # An argument crosses inside the Run payload, whose document and argument
  # list sit above it, so the deepest argument that arrives is two levels
  # shallower than the wire's bound.
  PAYLOAD_LEVELS = 2

  MEASURE = <<~RUBY
    Measure = ->(value) do
      depth = 0
      while value.is_a?(Array) && !value.empty?
        value = value[0]
        depth += 1
      end
      depth
    end
  RUBY

  # @behavior T-218
  def test_argument_whose_payload_reaches_the_bound_arrives_unchanged
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: MEASURE, name: :Measure)
    deepest = Kobako::Codec::MAX_NESTING_DEPTH - PAYLOAD_LEVELS

    depth = sandbox.run(:Measure, nested(deepest)).value

    assert_equal deepest, depth,
                 "a #run argument nested so the Run payload reaches the wire bound must arrive " \
                 "at the entrypoint nested to that depth"
  end

  # @behavior T-219
  def test_argument_a_level_past_the_payload_bound_is_refused_by_the_host
    too_deep = nested(Kobako::Codec::MAX_NESTING_DEPTH - PAYLOAD_LEVELS + 1)

    err = assert_raises(Kobako::SandboxError) { entry_sandbox.run(:App, too_deep) }

    assert_match(/nests deeper than 128 levels/, err.message,
                 "a #run argument nested one level past what the Run payload can carry must " \
                 "be refused by the host before the guest runs")
  end

  # @behavior T-219
  def test_keyword_value_a_level_past_the_payload_bound_is_refused_by_the_host
    too_deep = nested(Kobako::Codec::MAX_NESTING_DEPTH - PAYLOAD_LEVELS + 1)

    err = assert_raises(Kobako::SandboxError) { entry_sandbox.run(:App, value: too_deep) }

    assert_match(/nests deeper than 128 levels/, err.message,
                 "a #run keyword value nested one level past what the Run payload can carry " \
                 "must be refused by the host before the guest runs")
  end

  # Refusing a gadget among the arguments is what keeps every id the
  # invocation's Handle table holds one the guest was handed: the walk stops
  # at the gadget, and the run it was wrapping never reaches the guest, so no
  # id the walk had already issued is ever addressable.
  # @behavior T-222
  def test_reflective_gadget_argument_is_refused_before_the_guest_runs
    entered = []
    sandbox = probe_sandbox(-> { entered << true })

    err = assert_raises(Kobako::SandboxError) { sandbox.run(:App, Body.new("body"), Kernel) }

    assert_match(/a Module cannot cross as a Capability Handle/, err.message,
                 "a #run argument handing over host reflection must be refused by name, so " \
                 "the refusal is not mistaken for the sibling unwrappable-key one")
    assert_empty entered,
                 "a #run whose arguments hold a reflective gadget must fail before the " \
                 "entrypoint runs, so no reference the walk had already issued is addressable"
  end

  private

  # A list nesting +depth+ levels around an empty one.
  def nested(depth)
    (1..depth).reduce([]) { |inner, _| [inner] }
  end

  def entry_sandbox
    Kobako::Sandbox.new.tap { |sandbox| sandbox.preload(code: "App = ->(*, **) { nil }", name: :App) }
  end

  # A Sandbox whose entrypoint reports back to the host before it answers,
  # so a run that never reaches the guest is told apart from one that did.
  def probe_sandbox(probe)
    Kobako::Sandbox.new.tap do |sandbox|
      sandbox.bind("Probe::Entered", probe)
      sandbox.preload(code: "App = ->(*args) { Probe::Entered.call; args }", name: :App)
    end
  end
end
