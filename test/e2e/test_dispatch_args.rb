# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the transport (guest→host dispatch) value path through
# real mruby: kwargs symbolization at the dispatch boundary (E-15), rejection
# of a dispatch argument with no wire representation (E-55), Symbol fidelity
# (ext 0x00), and native Array / Hash argument and return fidelity (Type
# Mapping #7-#8). The outcome-path counterpart lives in test_outcome_values.rb.
class TestE2EDispatchArgs < Minitest::Test
  include E2eGuestHelper

  # SPEC.md E-15: kwargs string keys → symbol keys at the dispatch boundary.
  def test_kwargs_string_keys_become_symbol_keys_at_dispatch_boundary
    klass = Class.new do
      define_method(:lookup) { |name:, region:| "#{region}/#{name}" }
    end
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Geo::Lookup", klass.new)

    result = sandbox.eval('Geo::Lookup.lookup(name: "alice", region: "us")')

    assert_equal "us/alice", result,
                 "E-15: wire kwargs str keys symbolized at dispatch boundary (SPEC.md E-15)"
  end

  # SPEC.md L1001 + E-15: empty kwargs path also exercised.
  def test_empty_kwargs_dispatch_to_no_kwargs_method
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Math::Pi", -> { 3.14 })

    result = sandbox.eval("Math::Pi.call")

    assert_equal 3.14, result,
                 "E-15: empty kwargs dispatches cleanly to a no-kwargs method (SPEC.md L1001)"
  end

  # A short method name and a short kwarg key are both mruby inline symbols,
  # which mruby unpacks through one shared per-VM name buffer. Reading the
  # kwarg key while building the request must not corrupt the already-read
  # method name: a +get+ dispatched with +auth:+ must arrive as +get+, not as
  # the kwarg key truncated to the method-name length.
  def test_short_method_name_survives_a_short_kwarg_key
    klass = Class.new do
      define_method(:get) { |id, auth:| "#{id}:#{auth}" }
    end
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Http::Client", klass.new)

    result = sandbox.eval('Http::Client.get("u", auth: "tok")')

    assert_equal "u:tok", result,
                 "a short method name dispatched with a short kwarg key must reach the host intact, not truncated"
  end

  # transport path: an unrepresentable value is rejected at the guest call site
  # rather than coerced — E-55 covers "a dispatch argument or kwargs value", so
  # both the positional walk (+unpack_args_kwargs+) and the trailing-Hash walk
  # (+extract_hash_kwargs+) must reject. RpcProbe's +to_s+ sentinel would
  # surface if the old coercion path were live; the raise happens in the guest
  # bridge before dispatch, so the Service never runs. Uniform with the
  # return-path rejection (E-06) pinned in test_outcome_values.rb.
  UNREPRESENTABLE_DISPATCH_CALLS = {
    "positional argument" => "Sym::Echo.call(RpcProbe.new)",
    "kwargs value" => "Sym::Echo.call(data: RpcProbe.new)"
  }.freeze

  def test_rpc_unrepresentable_arg_rejected_not_coerced
    UNREPRESENTABLE_DISPATCH_CALLS.each do |position, call|
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
      sandbox.bind("Sym::Echo", ->(*args, **kwargs) { args.first || kwargs })
      script = "class RpcProbe; def to_s; '<sentinel>'; end; end\n#{call}"
      err = assert_raises(Kobako::SandboxError) { sandbox.eval(script) }
      assert_match(/not a supported sandbox value type/, err.message,
                   "E-55: an unrepresentable #{position} must be rejected at the guest " \
                   "call site as Kobako::SandboxError, never coerced to an Object#to_s String")
    end
  end

  # SPEC.md → Wire Codec → Ext Types → ext 0x00: a Symbol transport argument
  # travels on the wire as an ext 0x00 frame and arrives at the Service
  # as a Ruby Symbol (not as the +to_s+ string form).
  def test_rpc_arg_symbol_arrives_as_symbol
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Sym::Echo", ->(arg) { arg.is_a?(Symbol) ? "sym:#{arg}" : "str:#{arg}" })

    result = sandbox.eval("Sym::Echo.call(:user_42)")

    assert_equal "sym:user_42", result,
                 "transport path: Symbol arg must arrive at the Service as a Ruby Symbol " \
                 "(ext 0x00), not as a String via Object#to_s"
  end

  # transport path: a Service returning an Array must reach the guest as an
  # mruby Array (callable methods like +#length+, +#first+), not as
  # +nil+. Reproduces the +examples/codemode+ failure where
  # +KV::Store.keys+ — an +Array+ of +String+ — was deserialized to
  # +nil+ inside the guest.
  def test_rpc_service_returning_array_arrives_as_array_in_guest
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Keys", -> { %w[a b c] })

    result = sandbox.eval("KV::Keys.call.length")

    assert_equal 3, result,
                 "transport path: Service-returned Array must materialize as an mruby Array " \
                 "in the guest (currently regressed to nil — see codemode failure)"
  end

  # transport path: a Service returning a Hash must reach the guest as an
  # mruby Hash with usable subscript access; Symbol keys returned by
  # the host arrive as Symbols on the guest side.
  def test_rpc_service_returning_hash_arrives_as_hash_in_guest
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Snapshot", -> { { a: 1, b: 2 } })

    result = sandbox.eval("KV::Snapshot.call[:a]")

    assert_equal 1, result,
                 "transport path: Service-returned Hash must materialize as an mruby Hash " \
                 "with Symbol keys preserved (SPEC.md Type Mapping #8)"
  end

  # transport path: nested Array of Hash passes from guest → host → guest with
  # element-level fidelity. The Service captures into +seen+ before
  # echoing so the assertion can prove both the host-side arrival shape
  # and the guest-side round-trip shape match the original structure.
  NESTED_AOH = [{ x: 1 }, { y: 2 }].freeze

  def test_rpc_nested_array_of_hash_round_trip
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    seen = []
    sandbox.bind("Echo::Identity", ->(arg) { arg.tap { seen << arg } })

    result = sandbox.eval("Echo::Identity.call([{x: 1}, {y: 2}])")

    assert_equal NESTED_AOH, seen.first, "transport arg: nested Array-of-Hash must arrive natively"
    assert_equal NESTED_AOH, result, "transport return: nested Array-of-Hash must round-trip losslessly"
  end

  # transport path: argument conversion sizes a buffer from the array length,
  # so it reads the C element count rather than dispatching `#length`, which
  # untrusted guest mruby can override per-instance. An array whose `length`
  # the guest has inflated must still convert by its real elements — neither
  # trapping on an oversized reservation nor mis-shaping the request.
  OVERRIDDEN_LENGTH_SCRIPT = <<~RUBY
    a = [1, 2, 3]
    def a.length; 1_000_000_000; end
    Echo::Identity.call(a)
  RUBY

  def test_rpc_array_arg_ignores_guest_overridden_length
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    seen = []
    sandbox.bind("Echo::Identity", ->(arg) { arg.tap { seen << arg } })

    result = sandbox.eval(OVERRIDDEN_LENGTH_SCRIPT)

    assert_equal [1, 2, 3], seen.first,
                 "transport arg: a guest-overridden Array#length must not steer the conversion — " \
                 "the Service receives the real elements"
    assert_equal [1, 2, 3], result,
                 "transport return: the array round-trips by its real element count"
  end
end
