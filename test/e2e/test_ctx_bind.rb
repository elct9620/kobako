# frozen_string_literal: true

require "test_helper"

# E2E — the per-eval override block. #eval / #run yield a Context
# before the guest drives, so `ctx.bind` fills a fillable or shadows any
# declared binding for that one invocation, without touching Frame 1.
class TestE2ECtxBind < Minitest::Test
  include E2eGuestHelper

  # A minimal host store the guest reaches as the bound constant.
  class Kv
    def initialize(value) = (@value = value)
    def get(_key) = @value
  end

  # @behavior SV-024
  def test_ctx_bind_fills_a_fillable_for_the_invocation
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Store")

    result = sandbox.eval("Store.get(1)") { |ctx| ctx.bind("Store", Kv.new("filled")) }.value

    assert_equal "filled", result,
                 "ctx.bind must fill a fillable so the guest dispatch reaches the supplied object"
  end

  # @behavior SV-025
  def test_ctx_bind_fills_a_fillable_on_the_run_path
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.preload(code: "Worker = ->(*_a, **_k) { Store.get(1) }", name: :Worker)
    sandbox.bind("Store")

    result = sandbox.run(:Worker) { |ctx| ctx.bind("Store", Kv.new("filled")) }.value

    assert_equal "filled", result,
                 "ctx.bind on the #run path must fill a fillable so the entrypoint reaches the object"
  end

  # @behavior SV-026 SV-027
  def test_ctx_bind_shadows_a_static_binding_for_one_invocation_only
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Store", Kv.new("base"))

    overridden = sandbox.eval("Store.get(1)") { |ctx| ctx.bind("Store", Kv.new("override")) }.value
    plain = sandbox.eval("Store.get(1)").value

    assert_equal "override", overridden,
                 "ctx.bind must shadow the static binding for this invocation"
    assert_equal "base", plain,
                 "the override lasts only its own invocation; the next eval sees the base binding"
  end

  # @behavior SV-028
  def test_an_unfilled_fillable_without_an_override_still_fails_closed
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Store")

    assert_raises(Kobako::ServiceError,
                  "a fillable the block leaves unfilled must still fail closed as ServiceError") do
      sandbox.eval("Store.get(1)") { |_ctx| nil }
    end
  end

  # @behavior SV-029
  # A captured ctx must raise rather than mutate a completed invocation.
  def test_a_ctx_captured_from_the_block_is_spent_afterward
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Store")
    escaped = nil

    sandbox.eval("1") { |ctx| escaped = ctx }

    assert_raises(ArgumentError,
                  "ctx.bind on a ctx whose block has returned must raise ArgumentError — the same " \
                  "API-misuse channel as an undeclared path") do
      escaped.bind("Store", Kv.new("late"))
    end
  end

  # @behavior SV-045
  # The override is the narrower statement, written for this one invocation,
  # so it wins over what the provider resolves for every invocation.
  def test_an_override_outranks_the_object_a_provider_yields
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    backend = Kobako::Extension::Backend.new(path: "Store", provider: -> { Kv.new("provided") })
    sandbox.install(Kobako::Extension.new(name: :Store, source: "", backend: backend))

    result = sandbox.eval("Store.get(1)") { |ctx| ctx.bind("Store", Kv.new("override")) }.value

    assert_equal "override", result,
                 "ctx.bind through #eval must outrank the object a per-invocation provider yields " \
                 "for the same path"
  end

  # @behavior SV-046 SV-047
  # The block is Host App code, so what it raises is the Host App's own and
  # reaches the caller as raised; the guest never starts.
  def test_a_raising_override_block_reaches_the_caller_and_the_guest_never_runs
    ran = []
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Ran", -> { ran << true })
    raised = KeyError.new("override unavailable")

    caught = assert_raises(KeyError) { sandbox.eval("Probe::Ran.call") { |_ctx| raise raised } }

    assert_same raised, caught,
                "an exception raised inside the #eval override block must reach the caller unchanged"
    assert_empty ran, "when the override block raises, the guest must not run"
  end

  # @behavior SV-023
  # Raising inside the block keeps the Frame 1 key set fixed; the guest never
  # runs.
  def test_ctx_bind_on_an_undeclared_path_raises_inside_the_block
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Store")

    assert_raises(ArgumentError,
                  "ctx.bind on a path never declared must raise inside the block, keeping Frame 1 fixed") do
      sandbox.eval("1") { |ctx| ctx.bind("Undeclared", Kv.new("x")) }
    end
  end
end
