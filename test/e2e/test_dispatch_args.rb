# frozen_string_literal: true

require "test_helper"

# E2E (Layer 4) — the transport (guest→host dispatch) value path through
# real mruby: kwargs symbolization at the dispatch boundary (E-15), rejection
# of a dispatch argument with no wire representation (E-55), Symbol fidelity
# (ext 0x00), and native Array / Hash argument and return fidelity (Type
# Mapping #7-#8). The outcome-path counterpart lives in test_outcome_values.rb.
class TestE2EDispatchArgs < Minitest::Test
  include E2eGuestHelper

  # @behavior T-138
  # The wire carries keyword names as strings, so the symbolization has
  # to happen at the boundary — this is the same observation the unit
  # witness makes, driven through real mruby.
  def test_kwargs_string_keys_become_symbol_keys_at_dispatch_boundary
    klass = Class.new do
      define_method(:lookup) { |name:, region:| "#{region}/#{name}" }
    end
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Geo::Lookup", klass.new)

    result = sandbox.eval('Geo::Lookup.lookup(name: "alice", region: "us")').value

    assert_equal "us/alice", result,
                 "E-15: wire kwargs str keys symbolized at dispatch boundary (SPEC.md E-15)"
  end

  # @behavior T-142
  def test_empty_kwargs_dispatch_to_no_kwargs_method
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Math::Pi", -> { 3.14 })

    result = sandbox.eval("Math::Pi.call").value

    assert_equal 3.14, result,
                 "E-15: empty kwargs dispatches cleanly to a no-kwargs method (SPEC.md L1001)"
  end

  # @behavior T-147
  # Both names are mruby inline symbols, which unpack through one shared
  # per-VM buffer — reading the keyword while building the request must
  # not overwrite the method name already read into it.
  def test_short_method_name_survives_a_short_kwarg_key
    klass = Class.new do
      define_method(:get) { |id, auth:| "#{id}:#{auth}" }
    end
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Http::Client", klass.new)

    result = sandbox.eval('Http::Client.get("u", auth: "tok")').value

    assert_equal "u:tok", result,
                 "a short method name dispatched with a short kwarg key must reach the host intact, not truncated"
  end

  # transport path: an unrepresentable value is rejected at the guest call site
  # rather than coerced — E-55 covers "a dispatch argument or kwargs value", so
  # both the positional walk (+unpack_args_kwargs+) and the keyword-bucket walk
  # (+extract_hash_kwargs+) must reject. RpcProbe's +to_s+ sentinel would
  # surface if the old coercion path were live; the raise happens in the guest
  # bridge before dispatch, so the Service never runs. Uniform with the
  # return-path rejection (E-06) pinned in test_outcome_values.rb.
  #
  # The guest class is pinned alongside the host class because the two do not
  # move together: every sandbox-origin failure maps to +Kobako::SandboxError+
  # whatever raised it, so only +klass+ can say the script was told it handed
  # over the wrong type rather than that the wire broke.
  UNREPRESENTABLE_DISPATCH_CALLS = {
    "positional argument" => "Sym::Echo.call(RpcProbe.new)",
    "kwargs value" => "Sym::Echo.call(data: RpcProbe.new)"
  }.freeze

  # @behavior T-148
  # Both argument positions are covered because a guard on one would
  # leave the other open, and the class has to match the one the yield
  # position raises for the same refusal — the script chose the value,
  # so it is the script's error rather than a transport fault.
  def test_rpc_unrepresentable_arg_rejected_not_coerced
    UNREPRESENTABLE_DISPATCH_CALLS.each do |position, call|
      err = dispatch_unrepresentable(call)
      assert_match(/not a supported sandbox value type/, err.message,
                   "E-55: an unrepresentable #{position} must be rejected at the guest " \
                   "call site as Kobako::SandboxError, never coerced to an Object#to_s String")
      assert_equal "TypeError", err.klass,
                   "E-55: an unrepresentable #{position} must reach the script as TypeError, " \
                   "the same class the yield position raises for the same refusal, rather " \
                   "than reporting a transport fault for a value the script chose"
    end
  end

  # Run +call+ against a bound Service with an unrepresentable RpcProbe in it
  # and hand back the failure it raised.
  def dispatch_unrepresentable(call)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Sym::Echo", ->(*args, **kwargs) { args.first || kwargs })
    script = "class RpcProbe; def to_s; '<sentinel>'; end; end\n#{call}"
    assert_raises(Kobako::SandboxError) { sandbox.eval(script) }
  end

  # @behavior T-149
  # A Symbol travels as its own wire frame rather than as text, so the
  # Service receives the Symbol the guest wrote and not its `to_s` form.
  def test_rpc_arg_symbol_arrives_as_symbol
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Sym::Echo", ->(arg) { arg.is_a?(Symbol) ? "sym:#{arg}" : "str:#{arg}" })

    result = sandbox.eval("Sym::Echo.call(:user_42)").value

    assert_equal "sym:user_42", result,
                 "transport path: Symbol arg must arrive at the Service as a Ruby Symbol " \
                 "(ext 0x00), not as a String via Object#to_s"
  end

  # @behavior T-150
  # Reproduces the codemode failure where a Service answering an Array
  # of String deserialized to +nil+ inside the guest, so the assertion
  # calls a method on the answer rather than comparing it.
  def test_rpc_service_returning_array_arrives_as_array_in_guest
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Keys", -> { %w[a b c] })

    result = sandbox.eval("KV::Keys.call.length").value

    assert_equal 3, result,
                 "transport path: Service-returned Array must materialize as an mruby Array " \
                 "in the guest (currently regressed to nil — see codemode failure)"
  end

  # @behavior T-151
  # Subscripting by the Symbol key is what shows both that the answer is
  # a guest Hash and that its keys survived as Symbols.
  def test_rpc_service_returning_hash_arrives_as_hash_in_guest
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("KV::Snapshot", -> { { a: 1, b: 2 } })

    result = sandbox.eval("KV::Snapshot.call[:a]").value

    assert_equal 1, result,
                 "transport path: Service-returned Hash must materialize as an mruby Hash " \
                 "with Symbol keys preserved (SPEC.md Type Mapping #8)"
  end

  # The Service captures into +seen+ before echoing, so one call shows
  # both the host-side arrival shape and the guest-side return shape —
  # a conversion correct in one direction only would pass either alone.
  NESTED_AOH = [{ x: 1 }, { y: 2 }].freeze

  # @behavior T-152
  def test_rpc_nested_array_of_hash_round_trip
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    seen = []
    sandbox.bind("Echo::Identity", ->(arg) { arg.tap { seen << arg } })

    result = sandbox.eval("Echo::Identity.call([{x: 1}, {y: 2}])").value

    assert_equal NESTED_AOH, seen.first, "transport arg: nested Array-of-Hash must arrive natively"
    assert_equal NESTED_AOH, result, "transport return: nested Array-of-Hash must round-trip losslessly"
  end

  # Argument conversion sizes a buffer from the array length, so it reads
  # the element count directly rather than dispatching `#length`, which
  # untrusted guest code can override per instance — an inflated one would
  # otherwise steer an oversized reservation.
  OVERRIDDEN_LENGTH_SCRIPT = <<~RUBY
    a = [1, 2, 3]
    def a.length; 1_000_000_000; end
    Echo::Identity.call(a)
  RUBY

  # @behavior T-153
  def test_rpc_array_arg_ignores_guest_overridden_length
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    seen = []
    sandbox.bind("Echo::Identity", ->(arg) { arg.tap { seen << arg } })

    result = sandbox.eval(OVERRIDDEN_LENGTH_SCRIPT).value

    assert_equal [1, 2, 3], seen.first,
                 "transport arg: a guest-overridden Array#length must not steer the conversion — " \
                 "the Service receives the real elements"
    assert_equal [1, 2, 3], result,
                 "transport return: the array round-trips by its real element count"
  end
end
