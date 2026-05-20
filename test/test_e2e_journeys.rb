# frozen_string_literal: true

require "test_helper"

# Layer 4 — End-to-end user journey tests (SPEC.md Testing Style Layer 4, L1001).
#
# Each test corresponds to one of the five journeys (J-01..J-05) defined in
# SPEC.md L146-218 and exercises the full Host App → `Sandbox#eval` →
# Service call → result return path through real mruby (`data/kobako.wasm`).
# Layer 4
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

  # SPEC.md L152-156: model-generated script calls a curated Member
  # and the Host App receives a deserialized return value.
  def test_j01_curated_capability_call_returns_deserialized_result
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:KV).bind(:Lookup, ->(key) { "value:#{key}" })

    result = sandbox.eval(<<~RUBY)
      KV::Lookup.call("user_42")
    RUBY

    assert_equal "value:user_42", result,
                 "J-01: model-generated script must receive deserialized Service result (SPEC.md L156)"
  end

  # SPEC.md L157: scripts with Ruby errors raise SandboxError.
  def test_j01_script_ruby_error_raises_sandbox_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval(<<~RUBY)
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
  # author needs to fix and forces blind debugging. The host-side decoder
  # already pins the Array-of-String type invariant via +Outcome::Panic+,
  # so this E2E only asserts the non-empty contract.
  def test_j01_script_ruby_error_exposes_mruby_backtrace
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval(<<~RUBY)
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
      sandbox.eval(<<~RUBY)
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
      sandbox.eval(<<~RUBY)
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

    a = sandbox.eval('Data::Fetch.call("a")')
    b = sandbox.eval('Data::Fetch.call("b")')

    assert_equal "record:a", a, "J-02: first run sees the binding"
    assert_equal "record:b", b, "J-02: subsequent run still sees the binding (SPEC.md L173)"
  end

  # SPEC.md L169 + B-04: developer reads Sandbox#stdout for guest puts/print
  # output AND the script's return value comes through the outcome envelope.
  # Both channels are independently observable.
  def test_j02_stdout_and_return_value_independently_observable
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval(<<~RUBY)
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
      crashing.eval('raise "buggy submission"')
    end

    result = surviving.eval("1 + 1")

    assert_equal 2, result,
                 "J-03: a crashed submission Sandbox must not affect another (SPEC.md L187)"
  end

  # ── J-04 — No-code platform evaluates user-defined expressions per request ──
  #
  # SPEC.md L193-204: Per-tenant Sandbox; each event triggers a Sandbox#eval
  # with a user expression; expression result drives downstream logic.

  def test_j04_user_expression_evaluates_to_value_for_filter_logic
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Event).bind(:Amount, -> { 150 })

    pass_branch = sandbox.eval("Event::Amount.call > 100")
    fail_branch = sandbox.eval("Event::Amount.call > 1000")

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
      sandbox_a.eval('raise "script-level fault"')
    end

    # SPEC.md L216: ServiceError — capability-level fault.
    err = assert_raises(Kobako::ServiceError) do
      sandbox_b.eval('Svc::Call.call("x")')
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

  # SPEC.md B-17: Service A returns stateful object → guest uses Handle as
  # next RPC target → chain works.
  def test_handle_chain_b17_service_returns_handle_used_as_target
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Factory).bind(:Make, ->(name) { Greeter.new(name) })

    result = sandbox.eval(<<~RUBY)
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

    sandbox.eval('Factory::Make.call("alice")')
    handle_id = sandbox.services.handle_table.alloc(:run_n_marker)
    assert sandbox.services.handle_table.include?(handle_id), "B-18 setup: id present in run N"

    sandbox.eval("1 + 1")

    refute sandbox.services.handle_table.include?(handle_id),
           "B-18: HandleTable must be fully reset at the start of run N+1 (SPEC.md L423)"
    assert_raises(Kobako::HandleTableError) { sandbox.services.handle_table.fetch(handle_id) }
  end

  # mruby's +puts+ on a capped channel may raise +IOError+ once the
  # WASI write is rejected. The rescue swallows that script-level
  # failure so these tests pin only the host-observable contract
  # (clipped bytes + predicate); whether the failure surfaces as a
  # raised error or a silently-short write is intentionally not pinned.
  OVERFLOW_SCRIPT = 'begin; puts "long enough to overflow the 5-byte cap"; rescue StandardError; end; 1'

  # Symmetric overflow script for the stderr channel — uses +$stderr.puts+
  # directly because +Kernel#warn+ would route through the same global
  # but adds nothing to the truncation observation.
  OVERFLOW_STDERR_SCRIPT =
    'begin; $stderr.puts "long enough to overflow the 5-byte cap"; rescue StandardError; end; 1'

  # SPEC.md B-04: output past +stdout_limit+ is clipped at the cap
  # boundary, +#stdout+ carries no truncation sentinel, and
  # +#stdout_truncated?+ flips to +true+. The cap is enforced inside the
  # WASI pipe — +#run+ still returns the script's last expression.
  def test_stdout_truncation_flag_when_output_exceeds_cap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, stdout_limit: 5)
    result = sandbox.eval(OVERFLOW_SCRIPT)

    assert_equal 1, result
    assert_operator sandbox.stdout.bytesize, :<=, 5
    refute_includes sandbox.stdout, "[truncated]"
    assert sandbox.stdout_truncated?
  end

  # SPEC.md B-03: truncation predicates reset together with the capture
  # buffers at the start of the next +#run+.
  def test_stdout_truncated_predicate_resets_between_runs
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, stdout_limit: 5)
    sandbox.eval(OVERFLOW_SCRIPT)
    assert sandbox.stdout_truncated?, "setup: first run must overflow the cap"

    sandbox.eval("nil")
    refute sandbox.stdout_truncated?, "B-03: stdout_truncated? must reset on the next run"
    assert_equal "", sandbox.stdout
  end

  # SPEC.md B-04: $stderr writes land in Sandbox#stderr, not Sandbox#stdout.
  # Covers the guest-side fd 2 path enabled by the mrblib/io.rb + ::IO bridge.
  # The equality assertion rejects install-time noise (e.g. mruby's +mrb_warn+
  # for a NULL super class) leaking onto fd 2 — the guest's own +$stderr.puts+
  # output is the only thing the channel may carry on this run.
  def test_stderr_puts_routes_to_stderr_channel
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval('$stderr.puts "diagnostic"; 1')

    assert_equal "diagnostic\n", sandbox.stderr,
                 "B-04: $stderr.puts must reach Sandbox#stderr exclusively"
    assert_empty sandbox.stdout,
                 "B-04: stderr writes must not bleed into Sandbox#stdout"
  end

  # SPEC.md B-04: Kernel#warn delegates through $stderr per mrblib/kernel.rb,
  # so warned bytes show up on Sandbox#stderr like any other stderr write.
  # The equality assertion also rejects install-time noise (e.g. mruby's
  # +mrb_warn+ for a NULL super class) leaking onto fd 2 — the guest's own
  # +warn+ output is the only thing the channel may carry on this run.
  def test_warn_routes_to_stderr_channel
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval('warn "caution"; 1')

    assert_equal "caution\n", sandbox.stderr,
                 "Kernel#warn must route only the guest message through $stderr"
    assert_empty sandbox.stdout,
                 "Kernel#warn must not bleed into stdout"
  end

  # SPEC.md B-04: Kernel#putc routes through $stdout, Integer arg writes a
  # single byte (c & 0xff). Pins alignment with mruby-io's mrblib/kernel.rb
  # putc surface (vendor/mruby/mrbgems/mruby-io/mrblib/kernel.rb:95-98).
  def test_putc_integer_writes_byte_to_stdout
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval("putc 65; 1")

    assert_equal "A", sandbox.stdout,
                 "Kernel#putc with Integer must write the byte (c & 0xff) to $stdout"
    assert_empty sandbox.stderr,
                 "Kernel#putc must not bleed into stderr"
  end

  # SPEC.md B-04: Kernel#putc with an Integer masks with +& 0xff+ before
  # writing — mirrors mruby-io's +io_putc+ in
  # vendor/mruby/mrbgems/mruby-io/src/io.c:1103. The companion test
  # +test_putc_integer_writes_byte_to_stdout+ uses +putc 65+ where the
  # mask is the identity; this one feeds +putc 321+ (321 & 0xff == 65)
  # so dropping the mask would silently write +"Ł"+-ish bytes
  # instead of +"A"+ and the assertion would catch the drift.
  def test_putc_integer_masks_byte
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval("putc 321; 1")

    assert_equal "A", sandbox.stdout,
                 "Kernel#putc with Integer must mask via (c & 0xff); 321 → 65 → 'A'"
    assert_empty sandbox.stderr,
                 "Kernel#putc must not bleed into stderr"
  end

  # SPEC.md B-04: Kernel#putc returns +nil+, not the argument — pinned
  # by mruby-io's mrblib/kernel.rb:95-98. The IO-level +IO#putc+
  # returns the original object; the Kernel delegator deliberately
  # drops it. If anyone collapses the Kernel#putc body back to a
  # one-liner delegate, IO#putc's +obj+ would bleed through and this
  # assertion catches the drift.
  def test_kernel_putc_returns_nil
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    result = sandbox.eval("putc 65")

    assert_nil result,
               "Kernel#putc must return nil (mruby-io alignment), not the obj that IO#putc returns"
    assert_equal "A", sandbox.stdout,
                 "putc 65 must still land on stdout"
  end

  # SPEC.md B-04: Kernel#putc with a String writes the first character.
  # Mruby is compiled without MRB_UTF8_STRING, so the first character is
  # the first byte — same behavior as mruby-io's non-UTF8 fallback path
  # (vendor/mruby/mrbgems/mruby-io/src/io.c:1125-1129).
  def test_putc_string_writes_first_character_to_stdout
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval('putc "Zed"; 1')

    assert_equal "Z", sandbox.stdout,
                 "Kernel#putc with String must write only the first character to $stdout"
    assert_empty sandbox.stderr,
                 "Kernel#putc must not bleed into stderr"
  end

  # SPEC.md B-04: Kernel#p writes inspect form to $stdout (not the raw to_s).
  # Pins the inspect-format invariant that distinguishes #p from #puts.
  def test_p_writes_inspect_form_to_stdout
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval("p({a: 1}); 1")

    assert_includes sandbox.stdout, "{a: 1}",
                    "Kernel#p must write Hash inspect form to stdout (mruby 4.0 shorthand)"
  end

  # Reassigning $stdout = $stderr at script time must redirect subsequent
  # Kernel#puts output to the stderr capture channel. This is the whole
  # reason Kernel delegators route through the assignable global instead
  # of hard-coded fd 1, and verifies that mrblib/kernel.rb honors the
  # late binding.
  def test_redirecting_stdout_to_stderr_routes_subsequent_puts
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.eval('$stdout = $stderr; puts "redirected"; 1')

    assert_includes sandbox.stderr, "redirected",
                    "Kernel#puts after `$stdout = $stderr` must follow the reassignment"
    refute_includes sandbox.stdout, "redirected",
                    "Original stdout channel must stay empty after redirection"
  end

  # Guest IO is scoped to the two captured descriptors; any other fd
  # raises ArgumentError at construction so the failure is loud rather
  # than a silent fwrite to a no-op stream.
  def test_io_new_rejects_unsupported_fd
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval('IO.new(99, "w")')
    end

    assert_includes err.message, "kobako IO only supports fd",
                    "io_initialize must raise ArgumentError citing the fd constraint"
  end

  # Mirror of fd validation for the mode argument — only "w" is
  # supported because mruby-io's read-path is intentionally out of
  # scope (see mrblib/io.rb class doc).
  def test_io_new_rejects_unsupported_mode
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval('IO.new(1, "r")')
    end

    assert_includes err.message, 'kobako IO only supports mode "w"',
                    "io_initialize must raise ArgumentError citing the mode constraint"
  end

  # Pins the io_fileno C bridge through a real run: STDOUT was
  # constructed with fd 1 in install_raw, so STDOUT.fileno must round
  # trip back to 1. STDERR.fileno mirrors with 2.
  def test_stdout_and_stderr_fileno_return_underlying_descriptor
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_equal 1, sandbox.eval("STDOUT.fileno")
    assert_equal 2, sandbox.eval("STDERR.fileno")
  end

  # Reassigning $stdout inside a #run must not bleed into the next
  # #run — each invocation rebuilds the mruby state and reinstalls
  # the globals, so a subsequent puts always lands on the host's
  # stdout channel. Pins this per-run-reset invariant explicitly
  # because the redirection test alone would not catch a regression
  # that made the reassignment persistent.
  def test_stdout_assignment_does_not_persist_across_runs
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    sandbox.eval('$stdout = $stderr; puts "first"; 1')
    assert_includes sandbox.stderr, "first", "setup: first run must redirect"

    sandbox.eval('puts "second"; 2')
    assert_includes sandbox.stdout, "second",
                    "second run must restore $stdout to the stdout channel"
    refute_includes sandbox.stderr, "second",
                    "second run must not inherit the previous run's $stdout reassignment"
  end

  # Symmetric to test_stdout_truncation_flag_when_output_exceeds_cap.
  # Cap is enforced inside the WASI pipe on fd 2; #stderr never contains
  # truncation sentinels.
  def test_stderr_truncation_flag_when_output_exceeds_cap
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, stderr_limit: 5)
    result = sandbox.eval(OVERFLOW_STDERR_SCRIPT)

    assert_equal 1, result
    assert_operator sandbox.stderr.bytesize, :<=, 5
    refute_includes sandbox.stderr, "[truncated]"
    assert sandbox.stderr_truncated?
  end

  # SPEC.md B-04: stdout buffer is per-run; second #run does not see first run's output.
  def test_stdout_is_per_run_b04
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    sandbox.eval('puts "first"; 1')
    assert_includes sandbox.stdout, "first"

    sandbox.eval('puts "second"; 2')
    refute_includes sandbox.stdout, "first",
                    "B-04: stdout must reset between runs (SPEC.md B-04 L264-270)"
    assert_includes sandbox.stdout, "second"
  end

  # SPEC.md E-14: a Handle whose entry has been replaced with the
  # +:disconnected+ sentinel surfaces as a Service-origin error on the
  # next dispatch through that handle. Full mruby round-trip: Service
  # Setup returns a pre-allocated RPC::Handle whose backing entry was
  # immediately marked disconnected; the mruby method call against that
  # handle dispatches against the disconnected sentinel and the host
  # observes a +Kobako::ServiceError::Disconnected+ carrying the
  # dispatcher's disconnected message.
  #
  # The guest's exception bridge (+wasm/kobako-wasm/src/boot.rs+) maps
  # the Response.error +type="disconnected"+ field onto the
  # +Kobako::ServiceError::Disconnected+ mruby class before +mrb_raise+,
  # so the class name propagates into the Panic envelope's +class+ field
  # and the host-side +Kobako::Outcome.panic_target_class+ selects the
  # Disconnected subclass (pinned in unit form by
  # +TestSandboxOutcomeDecoding#test_panic_envelope_with_disconnected_klass_dispatches_disconnected_subclass+).
  def test_e14_disconnected_handle_target_raises_disconnected_subclass
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.define(:Dc).bind(:Setup, disconnected_handle_setup_lambda(sandbox))

    err = assert_raises(Kobako::ServiceError::Disconnected) do
      sandbox.eval("handle = Dc::Setup.call\nhandle.any_method\n")
    end

    assert_kind_of Kobako::ServiceError, err,
                   "Disconnected must remain a ServiceError subclass"
    assert_equal "service", err.origin
    assert_equal "Kobako::ServiceError::Disconnected", err.klass
    assert_match(/disconnected/, err.message)
  end

  # E-14 setup helper: alloc a fresh Object in the live HandleTable,
  # immediately replace the entry with the +:disconnected+ sentinel, and
  # return the RPC::Handle so the bound Service can hand it back to mruby
  # for use as a target on the next RPC.
  def disconnected_handle_setup_lambda(sandbox)
    lambda do
      id = sandbox.services.handle_table.alloc(Object.new)
      sandbox.services.handle_table.mark_disconnected(id)
      Kobako::RPC::Handle.new(id)
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

  # H-1 regression: a Float returned from the guest must reach the host
  # bit-identical, not via `Float#to_s` + Rust `parse` (which used the
  # mruby %.16g formatter and could drop the last ULP on edge values).
  # `0.1 + 0.2` is the canonical witness: the IEEE-754 result is
  # `0.30000000000000004` and any narrower text formatting would lose
  # the trailing 4. Asserting bit equality via `==` is sufficient
  # because the codec encodes Float as msgpack `float 64`.
  def test_outcome_float_precision_round_trips_full_f64
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    result = sandbox.eval("0.1 + 0.2")
    assert_equal 0.30000000000000004, result,
                 "H-1: Float outcome must round-trip the full 64-bit payload"
  end

  # H-2 regression: an Integer must round-trip via the direct unbox
  # path, not the previous +to_s + parse+ pipeline that silently fell
  # back to 0 on parse failure. mruby's MRB_INT32 word-box reserves a
  # tag bit on wasm32, so the addressable Fixnum range is narrower than
  # i32; use 2^28 ± 1 as a representative magnitude that exercises the
  # signed 32-bit return path of `kobako_fixnum_value` without leaving
  # the Fixnum-tagged range.
  def test_outcome_integer_round_trips_via_direct_unbox
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    assert_equal 268_435_457, sandbox.eval("268_435_457")
    assert_equal(-268_435_457, sandbox.eval("-268_435_457"))
  end

  # H-3 regression: a user-defined `inspect` that raises must not
  # longjmp past the Rust frame doing wire conversion. The guest
  # wraps the inspect call in `mrb_protect_error`; on raise the
  # converter falls back to `"#<ClassName>"` and the host still
  # observes a clean outcome (no TrapError, no panic).
  EXPLODING_INSPECT_SCRIPT = <<~RUBY
    class Boom
      def inspect; raise "inspect blew up"; end
      def to_s;    "<boom-to-s>"; end
    end
    Boom.new
  RUBY

  def test_outcome_inspect_raise_is_caught_by_mrb_protect_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    result = sandbox.eval(EXPLODING_INSPECT_SCRIPT)
    assert_equal "#<Boom>", result,
                 "H-3: a raising inspect must surface the protected fallback, not a trap"
  end

  def test_outcome_envelope_unknown_type_uses_inspect_not_to_s
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval(PROBE_SCRIPT)

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

    result = sandbox.eval(RPC_PROBE_SCRIPT)

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

    result = sandbox.eval("Sym::Echo.call(:user_42)")

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

    result = sandbox.eval('[1, "a", :b]')

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

    result = sandbox.eval('{a: 1, "b" => 2}')

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

    assert_equal [], sandbox.eval("[]"),
                 "outcome path: empty Array must arrive as `[]`, not the inspect string"
  end

  def test_outcome_empty_hash_round_trips
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    assert_equal({}, sandbox.eval("{}"),
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

    result = sandbox.eval("KV::Keys.call.length")

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

    result = sandbox.eval("KV::Snapshot.call[:a]")

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

    result = sandbox.eval("Echo::Identity.call([{x: 1}, {y: 2}])")

    assert_equal NESTED_AOH, seen.first, "RPC arg: nested Array-of-Hash must arrive natively"
    assert_equal NESTED_AOH, result, "RPC return: nested Array-of-Hash must round-trip losslessly"
  end

  # ── Regexp — mruby-onig-regexp brings Onigmo-backed Regexp into the
  #    guest. These journeys cover the surface a guest script needs:
  #    literal compilation, +=~+ index return, +String#match+ → MatchData,
  #    and runtime +Regexp.new+. Regexp objects do NOT cross the
  #    host↔guest wire — guests use them internally and project to wire-
  #    compatible types (String / Integer / Array) before returning.

  def test_regexp_literal_eq_tilde_returns_match_index
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval('"hello-2026-mruby" =~ /\\d{4}/')

    assert_equal 6, result,
                 "Regexp: =~ must return the byte index of the first match"
  end

  def test_regexp_string_match_returns_capture_groups
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval('"abc123def".match(/(\\d+)/).to_a')

    assert_equal %w[123 123], result,
                 "Regexp: String#match must yield MatchData populated " \
                 "with the captured groups (full match + group 1)"
  end

  def test_regexp_runtime_compilation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval(<<~RUBY)
      pattern = Regexp.new("a(b+)c")
      pattern.match("xxabbbcxx")[1]
    RUBY

    assert_equal "bbb", result,
                 "Regexp.new: dynamic Regexp construction must round-trip " \
                 "through the host and yield captures inside the guest"
  end

  # +=~+ on a non-matching pattern must return +nil+, NOT 0 / -1 / false.
  # This is the contract guest scripts rely on to write idiomatic
  # +str =~ /pat/ or default+ conditionals; nil also has to round-trip
  # through the host wire as Ruby +nil+, not as +Integer 0+ (a likely
  # bug if the codec sees an unset +int+ field).
  def test_regexp_no_match_returns_nil
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval('"abc" =~ /\\d+/')

    assert_nil result, "Regexp: =~ must return nil when no match"
  end

  # An invalid pattern compiled at runtime is a guest-side Ruby error
  # (Onigmo's RegexpError), so it must surface to the host as
  # +Kobako::SandboxError+ — the same shape +raise "..."+ takes
  # (see test_j01_script_ruby_error_raises_sandbox_error). Onigmo's
  # error text mentions "invalid regular expression"; pin the substring
  # so a future encoding-related rewording surfaces here.
  def test_regexp_invalid_pattern_raises_sandbox_error
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    err = assert_raises(Kobako::SandboxError) do
      sandbox.eval('Regexp.new("(unclosed")')
    end

    assert_equal "sandbox", err.origin
    assert_match(/invalid regular expression/i, err.message,
                 "Regexp: invalid pattern must surface Onigmo's diagnostic")
  end

  # Onigmo's encoding tables (unicode.o, utf_8.o, etc.) are vendored
  # by mruby-onig-regexp's bundled Onigmo source and linked into
  # libmruby.a. A literal pattern matching a multibyte UTF-8 string
  # proves those tables made it through the autotools + libtool +
  # llvm-ar pipeline intact — a regression here would mean the build
  # silently dropped encoding objects (which has happened in earlier
  # iterations of this patch chain).
  def test_regexp_matches_utf8_string_literal
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)

    result = sandbox.eval('"abc漢字def".match(/(漢字)/).to_a')

    assert_equal %w[漢字 漢字], result,
                 "Regexp: UTF-8 string match must round-trip multibyte " \
                 "captures (proves Onigmo's encoding tables are linked)"
  end

  # SPEC.md B-01 / E-19: a wall-clock `timeout` cap interrupts an
  # infinite loop at the next guest safepoint after the deadline. The
  # cap raises `Kobako::TimeoutError`, which is a `Kobako::TrapError`
  # subclass — callers that only care about the unrecoverable outcome
  # can rescue the base class.
  def test_timeout_cap_traps_infinite_loop
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: 0.2)

    started = Time.now
    err = assert_raises(Kobako::TimeoutError) { sandbox.eval("loop { }") }
    elapsed = Time.now - started

    assert_kind_of Kobako::TrapError, err,
                   "TimeoutError must be a TrapError subclass per SPEC.md E-19"
    assert_operator elapsed, :<, 2.0,
                    "timeout must fire within the configured budget (epoch ticker latency aside)"
    assert_match(/timeout|wall-clock/i, err.message)
  end

  # SPEC.md B-01 / E-20: `memory_limit` traps when guest `memory.grow`
  # would push the per-invocation linear-memory delta past the cap.
  # The cap measures only the growth attributable to this invocation —
  # the mruby image's initial allocation and the watermark left by
  # prior invocations sit outside the budget — so a runaway script
  # that allocates far more than the cap still surfaces as a trap.
  def test_memory_limit_cap_traps_runaway_allocation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 2 << 20)

    err = assert_raises(Kobako::MemoryLimitError) do
      sandbox.eval('a = []; 200.times { a << ("x" * 100_000) }; nil')
    end

    assert_kind_of Kobako::TrapError, err,
                   "MemoryLimitError must be a TrapError subclass per SPEC.md E-20"
    assert_match(/memory_limit/, err.message)
  end

  # SPEC.md B-01 / E-20: `memory_limit` is a per-invocation delta cap,
  # re-anchored at the linear-memory size observed when each invocation
  # enters. The same Sandbox can therefore run back-to-back scripts
  # that each allocate well within the cap, even when their combined
  # high-water mark exceeds it — the watermark left by the first
  # invocation is folded into the second invocation's baseline rather
  # than being charged against its budget.
  def test_memory_limit_resets_per_invocation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, memory_limit: 1 << 20)

    assert_equal 200_000, sandbox.eval('a = "x" * 200_000; a.bytesize')
    assert_equal 200_000, sandbox.eval('a = "x" * 200_000; a.bytesize')
  end

  # SPEC.md B-01: `timeout: nil` and `memory_limit: nil` both disable
  # the corresponding cap. With caps off, a small script must complete
  # normally — the guards are dormant rather than always-firing.
  def test_disabled_caps_allow_normal_execution
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM, timeout: nil, memory_limit: nil)

    assert_nil sandbox.timeout
    assert_nil sandbox.memory_limit
    assert_equal 3, sandbox.eval("1 + 2")
  end

  # B-32: preloaded snippets replay in insertion order against the fresh
  # mrb_state before each invocation. The first snippet defines a top-
  # level constant; subsequent invocations on the same Sandbox observe
  # it because the snippet table re-runs on every #eval, not just once.
  def test_b32_preloaded_snippet_is_visible_to_eval
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "ANSWER = 42", name: :Answers)

    assert_equal 42, sandbox.eval("ANSWER")
  end

  def test_b32_preloaded_snippets_replay_across_invocations
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "ANSWER = 42", name: :Answers)

    assert_equal 42, sandbox.eval("ANSWER")
    assert_equal 42, sandbox.eval("ANSWER")
  end

  def test_b32_preloaded_snippets_replay_in_insertion_order
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "BASE = 10", name: :Alpha)
    sandbox.preload(code: "EXTENDED = BASE * 2", name: :Beta)

    assert_equal 20, sandbox.eval("EXTENDED")
  end

  # E-36: a preloaded snippet whose top-level expression raises during
  # replay surfaces as Kobako::SandboxError with the backtrace attributed
  # to the snippet's `(snippet:Name)` filename.
  def test_e36_preloaded_snippet_replay_failure_surfaces_as_sandbox_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: 'raise "broken at preload"', name: :Broken)

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("nil") }
    assert_match(/broken at preload/, err.message)
    assert err.backtrace_lines.any? { |line| line.include?("(snippet:Broken)") },
           "expected backtrace to reference (snippet:Broken), got #{err.backtrace_lines.inspect}"
  end

  # J-07 — Host App preloads a worker and dispatches many invocations.
  # SPEC.md L243-254: setup-once / dispatch-many pattern using #preload +
  # #run. Per-invocation isolation (B-03) means no state leaks between
  # successive #run calls on the same Sandbox.
  def test_j07_preload_worker_and_dispatch_many_requests
    sandbox = Kobako::Sandbox.new
    # B-31 (mruby C API limitation): kwargs land as a trailing positional
    # Hash, so entrypoints take a Hash parameter and unpack it themselves.
    # See test_sandbox_run.rb:test_b31_passes_keyword_args_as_trailing_positional_hash.
    sandbox.preload(
      code: "class Worker; def self.call(req, opts = {}); req * (opts[:multiplier] || 1); end; end",
      name: :Worker
    )

    assert_equal 2, sandbox.run(:Worker, 2)
    assert_equal 9, sandbox.run(:Worker, 3, multiplier: 3)
    assert_equal 20, sandbox.run(:Worker, 4, multiplier: 5)
  end

  # J-07 follow-up: #run and #eval interleave freely on the same Sandbox;
  # both verbs replay the snippet table from a fresh mrb_state.
  def test_j07_eval_and_run_interleave_with_isolated_state
    sandbox = Kobako::Sandbox.new
    sandbox.preload(code: "Worker = ->(n) { n * n }", name: :Worker)

    assert_equal 16, sandbox.run(:Worker, 4)
    assert_equal 16, sandbox.eval("Worker.call(3) + 7")
    assert_equal 25, sandbox.run(:Worker, 5)
  end

  # docs/behavior.md B-32 (binary: form): a precompiled RITE bytecode
  # blob registered via `#preload(binary:)` is replayed against the
  # fresh `mrb_state` before each invocation, exactly like a `code:`
  # form snippet. The constant defined by the bytecode is observable to
  # subsequent `#eval` calls.
  #
  # Fixture source: `test/fixtures/snippet_answers.rb` (literally
  # `ANSWERS = 42`), compiled with `mrbc -g` to embed a `debug_info`
  # section so the bytecode meets B-32's identity requirement.
  BYTECODE_FIXTURE_PATH = File.expand_path("fixtures/snippet_answers.mrb", __dir__)

  def test_b32_preloaded_binary_snippet_is_visible_to_eval
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(BYTECODE_FIXTURE_PATH))

    assert_equal 42, sandbox.eval("ANSWERS"),
                 "B-32 (binary: form): preloaded bytecode must contribute its " \
                 "top-level constants to subsequent #eval calls"
  end

  def test_b32_preloaded_binary_snippet_replays_across_invocations
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(BYTECODE_FIXTURE_PATH))

    assert_equal 42, sandbox.eval("ANSWERS")
    assert_equal 42, sandbox.eval("ANSWERS"),
                 "B-32: bytecode snippet must replay against every fresh mrb_state, " \
                 "not just the first invocation"
  end

  # docs/behavior.md E-37: bytecode whose RITE version mismatches the
  # guest's pinned version surfaces as Kobako::BytecodeError on the
  # first invocation's snippet replay. The wrong_version fixture takes
  # the valid bytecode and flips the version bytes ("0400" → "9999")
  # so the failure path triggers without depending on a future mruby
  # version bump.
  E37_FIXTURE_PATH = File.expand_path("fixtures/snippet_wrong_version.mrb", __dir__)

  def test_e37_bytecode_wrong_version_raises_bytecode_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(E37_FIXTURE_PATH))

    err = assert_raises(Kobako::BytecodeError) { sandbox.eval("nil") }
    assert_kind_of Kobako::SandboxError, err,
                   "BytecodeError must remain a SandboxError subclass"
    assert_equal "sandbox", err.origin
    assert_equal "Kobako::BytecodeError", err.klass
  end

  # docs/behavior.md E-38: bytecode body that fails structural parse
  # against the loaded IREP reader surfaces as Kobako::BytecodeError.
  # The corrupt fixture is a header-prefix truncation of the valid
  # bytecode — enough to pass the four-byte RITE ident check but short
  # enough that section parsing fails inside mruby's load path.
  E38_FIXTURE_PATH = File.expand_path("fixtures/snippet_corrupt.mrb", __dir__)

  def test_e38_bytecode_corrupt_body_raises_bytecode_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(E38_FIXTURE_PATH))

    err = assert_raises(Kobako::BytecodeError) { sandbox.eval("nil") }
    assert_kind_of Kobako::SandboxError, err
    assert_equal "Kobako::BytecodeError", err.klass
  end

  # docs/behavior.md E-36 (binary: form): bytecode that loads cleanly
  # but whose top-level expression raises at replay surfaces as
  # Kobako::SandboxError with the natural mruby class preserved — NOT
  # promoted to Kobako::BytecodeError, which is reserved for the two
  # structural failure modes (E-37 / E-38). The raise_boom fixture is
  # `raise "boom from snippet"` compiled with `mrbc -g`.
  #
  # Scope: this test pins the E-36 dispatch contract only — the spec
  # change broadens E-36 to cover binary form, and the regression risk
  # is the silent promotion to BytecodeError that the previous
  # implementation enforced unconditionally. Backtrace attribution for
  # binary form (whatever filename the bytecode's debug_info carries,
  # routed through mruby's own `pack_backtrace`) is inherited from
  # upstream and unchanged by the spec relaxation, so it is not
  # separately pinned here. The source-form companion at
  # `test_e36_preloaded_snippet_replay_failure_surfaces_as_sandbox_error`
  # exercises the parallel attribution path for the `(snippet:Name)`
  # ccontext filename, which is host-set rather than upstream-inherited.
  E36_BINARY_FIXTURE_PATH = File.expand_path("fixtures/snippet_raise_boom.mrb", __dir__)

  def test_e36_binary_form_replay_raise_is_sandbox_error_not_bytecode_error
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(E36_BINARY_FIXTURE_PATH))

    err = assert_raises(Kobako::SandboxError) { sandbox.eval("nil") }
    refute_kind_of Kobako::BytecodeError, err,
                   "E-36: a binary-form snippet that raises at top level is " \
                   "a replay failure, not a bytecode structural failure"
    assert_equal "RuntimeError", err.klass,
                 "E-36: the natural mruby exception class must survive replay"
    assert_equal "sandbox", err.origin
    assert_match(/boom from snippet/, err.message)
  end

  # docs/behavior.md B-32 (binary: form): bytecode emitted without
  # `mrbc -g` carries no `debug_info` section. Per the relaxed B-32 it
  # remains a legal payload — the guest loads it normally and the
  # snippet contributes its top-level effects to the fresh `mrb_state`.
  # Backtrace frames originating in the snippet are silently omitted
  # per upstream mruby semantics, but class / message / origin
  # attribution on raised exceptions remain intact. The no_debug
  # fixture is the same `ANSWERS = 42` source compiled with the debug
  # switch omitted.
  STRIPPED_BYTECODE_FIXTURE_PATH = File.expand_path("fixtures/snippet_no_debug.mrb", __dir__)

  def test_b32_stripped_bytecode_loads_and_contributes_top_level_effects
    sandbox = Kobako::Sandbox.new
    sandbox.preload(binary: File.binread(STRIPPED_BYTECODE_FIXTURE_PATH))

    assert_equal 42, sandbox.eval("ANSWERS"),
                 "B-32: bytecode without debug_info must still contribute " \
                 "top-level effects on the fresh mrb_state"
  end
end
