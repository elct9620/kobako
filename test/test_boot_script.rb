# frozen_string_literal: true

# Intentionally does NOT require "test_helper" — like test_wasm_crate.rb
# this test inspects build-time artifacts and does not load the Ruby
# Kobako module.
require "minitest/autorun"
require "ripper"

# E2E test for item #10 (Guest Binary boot script).
#
# Two tiers:
#
#   * Fast tier (always runs): syntactic structural assertions on the
#     embedded mruby boot script source. Verifies the file parses as
#     valid Ruby (Ripper.sexp returns non-nil), that no Ruby 3.x-only
#     features mruby 3.2 cannot accept are present (pattern matching,
#     rightward assignment, endless methods), and that the three boot
#     responsibilities (Service::Group::Member proxy install, stdout/
#     stderr drain hook, error handler) appear in the source.
#
#   * Real tier (gated KOBAKO_E2E_BUILD=1): would evaluate the boot
#     script inside a real mruby VM. Skipped today — the in-VM run is
#     covered by item #11 (build pipeline) once libmruby.a is in the
#     link graph. The skip message documents this so reviewers see the
#     gap is scoped, not forgotten.
#
# tmp/REFERENCE.md Ch.5 §Boot Script 三職責 enumerates the three
# responsibilities; this test mirrors that breakdown so any drift in
# the boot script forces a corresponding update here.
class TestBootScript < Minitest::Test
  PROJECT_ROOT = File.expand_path("..", __dir__)
  BOOT_RB = File.join(PROJECT_ROOT, "wasm", "kobako-wasm", "src", "boot.rb")

  def setup
    @source = File.read(BOOT_RB)
    # Strip line comments before applying the mruby-compatibility regex
    # guards. Comments may legitimately mention forbidden features (e.g.
    # the file's own preamble lists what it does not use), and matching
    # them would generate false positives. Line-by-line strip preserves
    # line numbers so failures still point to the right place.
    @code = @source.lines.map { |l| l.sub(/(?<!\\)#.*$/, "") }.join
  end

  def test_boot_rb_exists_and_non_empty
    assert File.file?(BOOT_RB), "wasm/kobako-wasm/src/boot.rb must exist (item #10)"
    refute_empty @source.strip, "boot.rb must not be empty"
  end

  def test_boot_rb_parses_as_valid_ruby
    sexp = Ripper.sexp(@source)
    refute_nil sexp,
               "boot.rb must parse as valid Ruby (Ripper.sexp returned nil) — " \
               "this also catches mruby-incompatible Ruby 3.x syntax that the " \
               "host parser would still accept; the more specific feature " \
               "guards below cover the cases Ripper does not flag."
  end

  # ----- Mruby 3.2 compatibility guards -----
  #
  # These are conservative regex guards — they reject obvious uses of
  # Ruby features mruby 3.2 does not accept. They are not full parser-
  # level guards; the ground truth is mruby actually compiling the
  # string, which is gated to KOBAKO_E2E_BUILD=1 + item #11.

  def test_boot_rb_has_no_pattern_matching
    refute_match(/^\s*case\b.*\bin\b/, @code,
                 "mruby 3.2 does not support `case ... in` pattern matching")
    refute_match(/=>\s*\{/, @code,
                 "mruby 3.2 does not support hash pattern matching with `=> {}`")
  end

  def test_boot_rb_has_no_rightward_assignment
    # Rightward assignment: `expr => var`. The strict form `=> [bareword]`
    # is the unambiguous case mruby 3.2 cannot parse.
    refute_match(/^\s*\S.*=>\s*[a-z_][a-z0-9_]*\s*$/m, @code,
                 "mruby 3.2 does not support rightward assignment (`expr => var`)")
  end

  def test_boot_rb_has_no_endless_methods
    refute_match(/^\s*def\s+\w+(?:\([^)]*\))?\s*=/, @code,
                 "mruby 3.2 does not support endless method syntax (`def f = expr`)")
  end

  def test_boot_rb_has_no_data_define
    refute_match(/Data\.define\b/, @code,
                 "mruby 3.2 does not have `Data.define` (Ruby 3.2+ stdlib)")
  end

  # ----- Three responsibilities (REFERENCE Ch.5 §Boot Script 三職責) -----

  def test_boot_rb_responsibility_1_state_init
    # Responsibility 1: capture $stdout / $stderr handles and ensure
    # Kobako module reachable.
    assert_match(/STDOUT_REF\s*=\s*\$stdout/, @source,
                 "boot.rb must stash a stable $stdout reference (responsibility 1)")
    assert_match(/STDERR_REF\s*=\s*\$stderr/, @source,
                 "boot.rb must stash a stable $stderr reference (responsibility 1)")
    assert_match(/^module Kobako\b/, @source,
                 "boot.rb must define / reopen module Kobako")
  end

  def test_boot_rb_responsibility_2_service_member_proxy
    # Responsibility 2: install Service::Group::Member proxy. The base
    # class is `Kobako::RPC` whose singleton-class `method_missing`
    # routes calls through the Guest RPC Client (REFERENCE Ch.5 §Boot
    # Script 預載).
    assert_match(/class\s+RPC\b/, @source,
                 "boot.rb must define Kobako::RPC base class")
    assert_match(/def\s+method_missing\b/, @source,
                 "boot.rb must define method_missing on Kobako::RPC singleton")
    assert_match(/def\s+respond_to_missing\?/, @source,
                 "boot.rb must pair method_missing with respond_to_missing?")
    assert_match(/Kobako\.__rpc_call__/, @source,
                 "method_missing must dispatch through Kobako.__rpc_call__ (the " \
                 "mruby C-bridge entry point declared in rpc_client.rs)")
    assert_match(/class\s+Handle\b/, @source,
                 "boot.rb must define Kobako::Handle for ext 0x01 wire form")
  end

  def test_boot_rb_responsibility_3_io_drain_hook
    # Responsibility 3: stdout / stderr drain hook. WASI delivers fd 1
    # / fd 2 directly to host buffers; this method exists so the Rust
    # outer driver can flush before writing the outcome envelope.
    assert_match(/def\s+self\.flush_io\b/, @source,
                 "boot.rb must expose Kobako::Boot.flush_io for the Rust driver " \
                 "to call before writing the outcome envelope (responsibility 3)")
    assert_match(/STDOUT_REF\.flush/, @source,
                 "flush_io must flush the stable $stdout handle")
    assert_match(/STDERR_REF\.flush/, @source,
                 "flush_io must flush the stable $stderr handle")
  end

  # ----- Real tier (gated) -----

  def test_real_tier_boot_script_runs_in_mruby_vm
    skip "needs item #11 build pipeline (libmruby.a) to evaluate the boot " \
         "script inside a real mruby VM; gated KOBAKO_E2E_BUILD=1 once " \
         "item #11 lands" unless ENV["KOBAKO_E2E_BUILD"] == "1"

    skip "item #11 has not landed yet — the wasm32-wasip1 build does not " \
         "yet link libmruby.a; this skip is intentional and documented in " \
         "the test's preamble."
  end
end
