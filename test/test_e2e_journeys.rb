# frozen_string_literal: true

require "test_helper"

# Layer 4 — End-to-end user journey tests (SPEC.md Testing Style Layer 4, L1001).
#
# Each test corresponds to one of the five journeys (J-01..J-05) defined in
# SPEC.md L146-218 and exercises the full Host App → `Sandbox#run` → Service
# call → result return path through real mruby (`data/kobako.wasm`). Layer 4
# mandates exercising the production Guest Binary so guest scripts are real
# Ruby; the host-side decoder / dispatcher branches that do not need a live
# Sandbox stay covered by the unit tests in `test_sandbox_errors.rb` and
# `test_rpc_dispatch.rb`.
#
# Build prerequisite: `bundle exec rake wasm:build` produces `data/kobako.wasm`
# from `wasm/kobako-wasm/` + `vendor/mruby/`. When the artifact is missing,
# every test in this file `skip`s with a clear message — see follow-up
# item #29 for re-enablement once the vendor toolchain build succeeds.
class TestE2EJourneys < Minitest::Test
  REAL_WASM = File.expand_path("../data/kobako.wasm", __dir__)

  # Stateful object handed to B-17 chain tests — Factory::Make returns a
  # Greeter, the guest then routes greet() to it directly.
  class Greeter
    def initialize(name) = (@name = name)
    def greet = "hi,#{@name}"
  end

  def setup
    skip "native ext not compiled (run `bundle exec rake compile`)" unless defined?(Kobako::Wasm::Instance)
    return if File.exist?(REAL_WASM)

    skip "data/kobako.wasm missing — run `bundle exec rake wasm:build` " \
         "(requires `rake vendor:setup` + `rake mruby:build` first; " \
         "tracked as follow-up #29)"
  end

  # ── J-01 — LLM agent author runs model-generated code with curated capabilities ──
  #
  # SPEC.md L146-158: The Host App declares Service namespaces; generated
  # scripts that exceed declared capabilities receive ServiceError; scripts
  # with Ruby errors raise SandboxError; Wasm-level failures raise TrapError.

  # SPEC.md L152-156: model-generated script calls a curated Service Member
  # and the Host App receives a deserialized return value.
  def test_j01_curated_capability_call_returns_deserialized_result
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:KV).bind(:Lookup, ->(key) { "value:#{key}" })

    result = sandbox.run(<<~RUBY)
      KV::Lookup.call("user_42")
    RUBY

    assert_equal "value:user_42", result,
                 "J-01: model-generated script must receive deserialized Service result (SPEC.md L156)"
  end

  # SPEC.md L157: scripts with Ruby errors raise SandboxError.
  def test_j01_script_ruby_error_raises_sandbox_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.run(<<~RUBY)
        raise "model produced bad code"
      RUBY
    end

    assert_equal "sandbox", err.origin
    refute_kind_of Kobako::ServiceError, err
    refute_kind_of Kobako::TrapError, err
  end

  # SPEC.md "Panic Envelope" L876 — the +backtrace+ field is an array of
  # str carrying the mruby backtrace. The guest must populate it from the
  # mruby Exception object so the Host App can see where the failure
  # originated inside the user script; an empty array hides which line the
  # author needs to fix and forces blind debugging. Wire layer already
  # pins the Array-of-String type invariant via +Wire::Envelope::Panic+,
  # so this E2E only asserts the non-empty contract.
  def test_j01_script_ruby_error_exposes_mruby_backtrace
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    err = assert_raises(Kobako::SandboxError) do
      sandbox.run(<<~RUBY)
        def boom
          raise "model produced bad code"
        end
        boom
      RUBY
    end
    refute_empty err.backtrace_lines, "SPEC L876: guest must populate Panic.backtrace"
  end

  # SPEC.md L157: Service capability call that errors → ServiceError.
  def test_j01_capability_error_raises_service_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Log).bind(:Sink, ->(_msg) { raise "capability denied" })

    err = assert_raises(Kobako::ServiceError) do
      sandbox.run(<<~RUBY)
        Log::Sink.call("secret")
      RUBY
    end

    assert_equal "service", err.origin
    refute_kind_of Kobako::SandboxError, err
  end

  # SPEC.md L876 again — an unrescued Service call equally flows through
  # the Panic envelope, so its backtrace must also reach the Host App.
  # Otherwise an LLM-generated script that calls a misbehaving capability
  # would surface as ServiceError with no debugging context at all.
  def test_j01_unrescued_service_error_exposes_mruby_backtrace
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Log).bind(:Sink, ->(_msg) { raise "capability denied" })

    err = assert_raises(Kobako::ServiceError) do
      sandbox.run(<<~RUBY)
        Log::Sink.call("secret")
      RUBY
    end

    refute_empty err.backtrace_lines,
                 "guest must populate Panic.backtrace for service-origin panics too"
  end

  # ── J-02 — Host App developer integrates kobako into an existing service ──
  #
  # SPEC.md L161-173: Setup-once / run-many pattern; same Sandbox resets
  # capability state between #run calls; Service objects bound at setup
  # time remain active across runs without re-registration.

  def test_j02_setup_once_run_many_with_persistent_service_bindings
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Data).bind(:Fetch, ->(id) { "record:#{id}" })

    a = sandbox.run('Data::Fetch.call("a")')
    b = sandbox.run('Data::Fetch.call("b")')

    assert_equal "record:a", a, "J-02: first run sees the binding"
    assert_equal "record:b", b, "J-02: subsequent run still sees the binding (SPEC.md L173)"
  end

  # SPEC.md L169 + B-04: developer reads Sandbox#stdout for guest puts/print
  # output AND the script's return value comes through the outcome envelope.
  # Both channels are independently observable.
  def test_j02_stdout_and_return_value_independently_observable
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.run(<<~RUBY)
      puts "diagnostic"
      42
    RUBY

    assert_equal 42, result,
                 "J-02 / B-04: return value comes through outcome envelope, not stdout"
    assert_includes sandbox.stdout, "diagnostic",
                    "J-02 / B-04: guest puts is captured in Sandbox#stdout (SPEC.md L169, B-04)"
  end

  # ── J-03 — Teaching platform evaluates student submissions in isolation ──
  #
  # SPEC.md L177-189: Each submission runs in a fresh Sandbox; a failing
  # submission must not affect another submission. No submission can read
  # another submission's guest output.

  def test_j03_fresh_sandbox_per_submission_isolates_failure
    crashing  = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    surviving = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_raises(Kobako::SandboxError) do
      crashing.run('raise "buggy submission"')
    end

    result = surviving.run("1 + 1")

    assert_equal 2, result,
                 "J-03: a crashed submission Sandbox must not affect another (SPEC.md L187)"
  end

  # ── J-04 — No-code platform evaluates user-defined expressions per request ──
  #
  # SPEC.md L193-204: Per-tenant Sandbox; each event triggers a Sandbox#run
  # with a user expression; expression result drives downstream logic.

  def test_j04_user_expression_evaluates_to_value_for_filter_logic
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Event).bind(:Amount, -> { 150 })

    pass_branch = sandbox.run("Event::Amount.call > 100")
    fail_branch = sandbox.run("Event::Amount.call > 1000")

    assert_equal true,  pass_branch, "J-04: user expression evaluates to true (SPEC.md L201)"
    assert_equal false, fail_branch, "J-04: user expression evaluates to false (SPEC.md L201)"
  end

  # ── J-05 — Host App developer distinguishes and handles the three error classes ──
  #
  # SPEC.md L208-220: The three-class taxonomy lets the developer route
  # each failure class through existing error-handling infrastructure.

  def test_j05_developer_distinguishes_three_error_classes
    sandbox_a = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox_b = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox_b.define(:Svc).bind(:Call, ->(_) { raise "service exploded" })

    # SPEC.md L215: SandboxError — script-level fault.
    assert_raises(Kobako::SandboxError) do
      sandbox_a.run('raise "script-level fault"')
    end

    # SPEC.md L216: ServiceError — capability-level fault.
    err = assert_raises(Kobako::ServiceError) do
      sandbox_b.run('Svc::Call.call("x")')
    end
    assert_equal "service", err.origin
  end

  # ── Layer 4 mandated coverage (SPEC.md L1001) ─────────────────────────────
  #
  # The seven mandated scenarios — kwargs dispatch (E-15), Handle chaining
  # (B-17), cross-run Handle invalidity (B-18 + E-13), stdout/stderr isolation
  # (B-04) — must be covered through real mruby at this layer. Wire-violation
  # edge cases (host-side decode paths) are Layer 3 tests housed in
  # `test_sandbox_errors.rb` (TestSandboxOutcomeDecoding).

  # SPEC.md E-15: kwargs string keys → symbol keys at the dispatch boundary.
  def test_kwargs_string_keys_become_symbol_keys_at_dispatch_boundary
    klass = Class.new do
      define_method(:lookup) { |name:, region:| "#{region}/#{name}" }
    end
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Geo).bind(:Lookup, klass.new)

    result = sandbox.run('Geo::Lookup.lookup(name: "alice", region: "us")')

    assert_equal "us/alice", result,
                 "E-15: wire kwargs str keys symbolized at dispatch boundary (SPEC.md E-15)"
  end

  # SPEC.md L1001 + E-15: empty kwargs path also exercised.
  def test_empty_kwargs_dispatch_to_no_kwargs_method
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Math).bind(:Pi, -> { 3.14 })

    result = sandbox.run("Math::Pi.call")

    assert_equal 3.14, result,
                 "E-15: empty kwargs dispatches cleanly to a no-kwargs method (SPEC.md L1001)"
  end

  # SPEC.md B-17: Service A returns stateful object → guest uses Handle as
  # next RPC target → chain works.
  def test_handle_chain_b17_service_returns_handle_used_as_target
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Factory).bind(:Make, ->(name) { Greeter.new(name) })

    result = sandbox.run(<<~RUBY)
      g = Factory::Make.call("Bob")
      g.greet
    RUBY

    assert_equal "hi,Bob", result,
                 "B-17: Handle target from first RPC routes second RPC to the stateful object"
  end

  # SPEC.md B-18 + E-13: cross-run Handle invalidity. A Handle obtained in
  # run N must not be reachable in run N+1 — the HandleTable is fully reset.
  def test_cross_run_handle_invalidity_b18_e13
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Factory).bind(:Make, ->(_n) { Object.new })

    sandbox.run('Factory::Make.call("alice")')
    handle_id = sandbox.services.handle_table.alloc(:run_n_marker)
    assert sandbox.services.handle_table.include?(handle_id), "B-18 setup: id present in run N"

    sandbox.run("1 + 1")

    refute sandbox.services.handle_table.include?(handle_id),
           "B-18: HandleTable must be fully reset at the start of run N+1 (SPEC.md L423)"
    assert_raises(Kobako::HandleTableError) { sandbox.services.handle_table.fetch(handle_id) }
  end

  # SPEC.md B-04: output past +stdout_limit+ is truncated with a
  # +[truncated]+ marker rather than raising; the cap is enforced even
  # under real WASI capture from the mruby guest.
  def test_stdout_truncation_marker_when_output_exceeds_cap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, stdout_limit: 5)
    sandbox.run('puts "long enough to overflow the 5-byte cap"; 1')
    assert_includes sandbox.stdout, "[truncated]"
  end

  # SPEC.md B-04: stdout buffer is per-run; second #run does not see first run's output.
  def test_stdout_buffer_is_per_run_b04
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    sandbox.run('puts "first"; 1')
    assert_includes sandbox.stdout, "first"

    sandbox.run('puts "second"; 2')
    refute_includes sandbox.stdout, "first",
                    "B-04: stdout must reset between runs (SPEC.md B-04 L264-270)"
    assert_includes sandbox.stdout, "second"
  end

  # SPEC.md E-14: a Handle whose entry has been replaced with the
  # +:disconnected+ sentinel surfaces as a Service-origin error on the
  # next dispatch through that handle. Full mruby round-trip: Service
  # Setup returns a pre-allocated Wire::Handle whose backing entry was
  # immediately marked disconnected; the mruby method call against that
  # handle dispatches against the disconnected sentinel and the host
  # observes a +Kobako::ServiceError::Disconnected+ carrying the
  # dispatcher's disconnected message.
  #
  # The guest's exception bridge (+wasm/kobako-wasm/src/boot.rs+) maps
  # the Response.err +type="disconnected"+ field onto the
  # +Kobako::ServiceError::Disconnected+ mruby class before +mrb_raise+,
  # so the class name propagates into the Panic envelope's +class+ field
  # and the host-side +OutcomeDecoder.panic_target_class+ selects the
  # Disconnected subclass (pinned in unit form by
  # +TestSandboxOutcomeDecoding#test_panic_envelope_with_disconnected_klass_dispatches_disconnected_subclass+).
  def test_e14_disconnected_handle_target_raises_disconnected_subclass
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Dc).bind(:Setup, disconnected_handle_setup_lambda(sandbox))

    err = assert_raises(Kobako::ServiceError::Disconnected) do
      sandbox.run("handle = Dc::Setup.call\nhandle.any_method\n")
    end

    assert_kind_of Kobako::ServiceError, err,
                   "Disconnected must remain a ServiceError subclass"
    assert_equal "service", err.origin
    assert_equal "Kobako::ServiceError::Disconnected", err.klass
    assert_match(/disconnected/, err.message)
  end

  # E-14 setup helper: alloc a fresh Object in the live HandleTable,
  # immediately replace the entry with the +:disconnected+ sentinel, and
  # return the Wire::Handle so the bound Service can hand it back to mruby
  # for use as a target on the next RPC.
  def disconnected_handle_setup_lambda(sandbox)
    lambda do
      id = sandbox.services.handle_table.alloc(Object.new)
      sandbox.services.handle_table.mark_disconnected(id)
      Kobako::Wire::Handle.new(id)
    end
  end

  # ── Wire converter contract guards ─────────────────────────────────────
  #
  # +Kobako::mrb_value_to_wire_outcome+ (outcome path, +inspect+ fallback)
  # and +Kobako::mrb_value_to_wire_value+ (RPC path, +to_s+ fallback)
  # intentionally diverge; see the cross-referenced doc-comments on both
  # methods in +wasm/kobako-wasm/src/kobako.rs+. The two tests below pin
  # the divergence — one per direction — so a future "DRY cleanup" that
  # unifies them fails loudly on whichever side regressed.

  # Outcome path: the unknown-type fallback arm uses +Object#inspect+,
  # NOT +Object#to_s+. The Probe class defined inside the script
  # overrides both with distinct strings; if the converter switched to
  # +to_s+, this assertion would surface +"<probe-to-s>"+ instead of
  # +"<probe-inspect>"+.
  PROBE_SCRIPT = <<~RUBY
    class Probe
      def inspect; "<probe-inspect>"; end
      def to_s;    "<probe-to-s>";    end
    end
    Probe.new
  RUBY

  def test_outcome_envelope_unknown_type_uses_inspect_not_to_s
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.run(PROBE_SCRIPT)

    assert_equal "<probe-inspect>", result,
                 "outcome path: unknown-type fallback must call Object#inspect — " \
                 "see Kobako::mrb_value_to_wire_outcome doc"
  end

  # RPC path: the unknown-type fallback arm uses +Object#to_s+, NOT
  # +Object#inspect+. A user-defined mruby class is not in
  # +mrb_value_to_wire_value+'s named arms (NilClass / Bool / Integer /
  # Float / String / Symbol), so it falls through the +to_s+ fallback,
  # arrives at the Service as a plain String, and is echoed back. If
  # the converter switched to +inspect+, this assertion would surface
  # +"<rpc-probe-inspect>"+ instead of +"<rpc-probe-to-s>"+.
  RPC_PROBE_SCRIPT = <<~RUBY
    class RpcProbe
      def inspect; "<rpc-probe-inspect>"; end
      def to_s;    "<rpc-probe-to-s>";    end
    end
    Sym::Echo.call(RpcProbe.new)
  RUBY

  def test_rpc_arg_unknown_type_uses_to_s_not_inspect
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Sym).bind(:Echo, ->(arg) { arg })

    result = sandbox.run(RPC_PROBE_SCRIPT)

    assert_equal "<rpc-probe-to-s>", result,
                 "RPC path: unknown-type fallback must call Object#to_s — " \
                 "see Kobako::mrb_value_to_wire_value doc"
  end

  # SPEC.md → Wire Codec → Ext Types → ext 0x00: a Symbol RPC argument
  # travels on the wire as an ext 0x00 frame and arrives at the Service
  # as a Ruby Symbol (not as the +to_s+ string form).
  def test_rpc_arg_symbol_arrives_as_symbol
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Sym).bind(:Echo, ->(arg) { arg.is_a?(Symbol) ? "sym:#{arg}" : "str:#{arg}" })

    result = sandbox.run("Sym::Echo.call(:user_42)")

    assert_equal "sym:user_42", result,
                 "RPC path: Symbol arg must arrive at the Service as a Ruby Symbol " \
                 "(ext 0x00), not as a String via Object#to_s"
  end

  # ── Native Array / Hash round-trips (SPEC.md Type Mapping #7-#8) ──────
  #
  # The 12-entry Type Mapping (SPEC.md → Wire Codec → Type Mapping) maps
  # msgpack array → mruby Array and msgpack map → mruby Hash. Both
  # directions must travel by value with element-level fidelity (SPEC.md
  # B-13: "Collections (Array, Hash) whose elements are all
  # wire-representable are transmitted in full by value"). These tests
  # pin the guarantee through the real guest binary.

  # Outcome path: a script whose last expression is an mruby Array must
  # serialize as +Value::Array+ on the wire, not as the +inspect+
  # string. Mixed-element fidelity (Integer + String + Symbol) is part
  # of the contract.
  def test_outcome_array_returns_native_array
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.run('[1, "a", :b]')

    assert_equal [1, "a", :b], result,
                 "outcome path: mruby Array must arrive as a Ruby Array with " \
                 "preserved element types (SPEC.md Type Mapping #7)"
  end

  # Outcome path: an mruby Hash must serialize as +Value::Map+ and
  # arrive as a Ruby Hash. Symbol-vs-String key distinction is part of
  # the wire contract — SPEC.md Ext Types pins that
  # +"a"+ and +:a+ are not wire-equivalent.
  def test_outcome_hash_returns_native_hash
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.run('{a: 1, "b" => 2}')

    assert_equal({ a: 1, "b" => 2 }, result,
                 "outcome path: mruby Hash must arrive as a Ruby Hash preserving " \
                 "the Symbol-vs-String key distinction (SPEC.md Type Mapping #8 + ext 0x00)")
  end

  # Empty collection round-trips. The previous converter had a
  # +"Hash" => "{}"+ string sentinel for the empty-Hash case; this
  # commit's predecessor removed it on the premise that
  # +Value::Map(vec![])+ is the canonical wire encoding for an empty
  # Hash. These two tests pin the canonical encoding end-to-end so any
  # regression that re-introduces an empty-sentinel string surfaces
  # immediately.
  def test_outcome_empty_array_round_trips
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_equal [], sandbox.run("[]"),
                 "outcome path: empty Array must arrive as `[]`, not the inspect string"
  end

  def test_outcome_empty_hash_round_trips
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_equal({}, sandbox.run("{}"),
                 "outcome path: empty Hash must arrive as `{}`, not the legacy `\"{}\"` sentinel")
  end

  # RPC path: a Service returning an Array must reach the guest as an
  # mruby Array (callable methods like +#length+, +#first+), not as
  # +nil+. Reproduces the +examples/codemode+ failure where
  # +KV::Store.keys+ — an +Array+ of +String+ — was deserialized to
  # +nil+ inside the guest.
  def test_rpc_service_returning_array_arrives_as_array_in_guest
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:KV).bind(:Keys, -> { %w[a b c] })

    result = sandbox.run("KV::Keys.call.length")

    assert_equal 3, result,
                 "RPC path: Service-returned Array must materialize as an mruby Array " \
                 "in the guest (currently regressed to nil — see codemode failure)"
  end

  # RPC path: a Service returning a Hash must reach the guest as an
  # mruby Hash with usable subscript access; Symbol keys returned by
  # the host arrive as Symbols on the guest side.
  def test_rpc_service_returning_hash_arrives_as_hash_in_guest
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:KV).bind(:Snapshot, -> { { a: 1, b: 2 } })

    result = sandbox.run("KV::Snapshot.call[:a]")

    assert_equal 1, result,
                 "RPC path: Service-returned Hash must materialize as an mruby Hash " \
                 "with Symbol keys preserved (SPEC.md Type Mapping #8)"
  end

  # RPC path: nested Array of Hash passes from guest → host → guest with
  # element-level fidelity. The Service captures into +seen+ before
  # echoing so the assertion can prove both the host-side arrival shape
  # and the guest-side round-trip shape match the original structure.
  NESTED_AOH = [{ x: 1 }, { y: 2 }].freeze

  def test_rpc_nested_array_of_hash_round_trip
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    seen = []
    sandbox.define(:Echo).bind(:Identity, ->(arg) { arg.tap { seen << arg } })

    result = sandbox.run("Echo::Identity.call([{x: 1}, {y: 2}])")

    assert_equal NESTED_AOH, seen.first, "RPC arg: nested Array-of-Hash must arrive natively"
    assert_equal NESTED_AOH, result, "RPC return: nested Array-of-Hash must round-trip losslessly"
  end
end
