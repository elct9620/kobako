# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the transport (guest→host dispatch) value path through
# real mruby: kwargs symbolization at the dispatch boundary (E-15), the
# +to_codec_value+ unknown-type +to_s+ fallback, Symbol fidelity (ext 0x00),
# and native Array / Hash argument and return fidelity (Type Mapping #7-#8).
# The outcome-path counterpart lives in test_outcome_values.rb.
class TestE2EDispatchArgs < Minitest::Test
  include E2eGuestHelper

  # SPEC.md E-15: kwargs string keys → symbol keys at the dispatch boundary.
  def test_kwargs_string_keys_become_symbol_keys_at_dispatch_boundary
    klass = Class.new do
      define_method(:lookup) { |name:, region:| "#{region}/#{name}" }
    end
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Geo).bind(:Lookup, klass.new)

    result = sandbox.eval('Geo::Lookup.lookup(name: "alice", region: "us")')

    assert_equal "us/alice", result,
                 "E-15: wire kwargs str keys symbolized at dispatch boundary (SPEC.md E-15)"
  end

  # SPEC.md L1001 + E-15: empty kwargs path also exercised.
  def test_empty_kwargs_dispatch_to_no_kwargs_method
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Math).bind(:Pi, -> { 3.14 })

    result = sandbox.eval("Math::Pi.call")

    assert_equal 3.14, result,
                 "E-15: empty kwargs dispatches cleanly to a no-kwargs method (SPEC.md L1001)"
  end

  # transport path: the unknown-type fallback arm uses +Object#to_s+, NOT
  # +Object#inspect+. A user-defined mruby class is not in
  # +to_codec_value+'s named arms (NilClass / Bool / Integer / Float /
  # String / Symbol), so it falls through the +to_s+ fallback, arrives at
  # the Service as a plain String, and is echoed back. If the converter
  # switched to +inspect+, this assertion would surface
  # +"<rpc-probe-inspect>"+ instead of +"<rpc-probe-to-s>"+. The outcome
  # path deliberately diverges (raise instead of coerce, E-06) — its pin
  # lives in test_outcome_values.rb so a "DRY cleanup" that unifies the
  # two converters fails loudly.
  TRANSPORT_PROBE_SCRIPT = <<~RUBY
    class RpcProbe
      def inspect; "<rpc-probe-inspect>"; end
      def to_s;    "<rpc-probe-to-s>";    end
    end
    Sym::Echo.call(RpcProbe.new)
  RUBY

  def test_rpc_arg_unknown_type_uses_to_s_not_inspect
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Sym).bind(:Echo, ->(arg) { arg })

    result = sandbox.eval(TRANSPORT_PROBE_SCRIPT)

    assert_equal "<rpc-probe-to-s>", result,
                 "transport path: unknown-type fallback must call Object#to_s — " \
                 "see Kobako::to_codec_value doc"
  end

  # SPEC.md → Wire Codec → Ext Types → ext 0x00: a Symbol transport argument
  # travels on the wire as an ext 0x00 frame and arrives at the Service
  # as a Ruby Symbol (not as the +to_s+ string form).
  def test_rpc_arg_symbol_arrives_as_symbol
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Sym).bind(:Echo, ->(arg) { arg.is_a?(Symbol) ? "sym:#{arg}" : "str:#{arg}" })

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
    sandbox.define(:KV).bind(:Keys, -> { %w[a b c] })

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
    sandbox.define(:KV).bind(:Snapshot, -> { { a: 1, b: 2 } })

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
    sandbox.define(:Echo).bind(:Identity, ->(arg) { arg.tap { seen << arg } })

    result = sandbox.eval("Echo::Identity.call([{x: 1}, {y: 2}])")

    assert_equal NESTED_AOH, seen.first, "transport arg: nested Array-of-Hash must arrive natively"
    assert_equal NESTED_AOH, result, "transport return: nested Array-of-Hash must round-trip losslessly"
  end
end
