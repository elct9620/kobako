# frozen_string_literal: true

require "test_helper"

require_relative "../../benchmark/support/guest"

# The injection seam +bench:confirm+ relies on to run an arm against a
# baseline Guest Binary without swapping data/kobako.wasm: KOBAKO_BENCH_WASM
# overrides the probe's own default, and its absence leaves that default
# in force.
class KobakoBenchGuestTest < Minitest::Test
  def setup
    @saved = ENV.fetch(Kobako::Bench::Guest::ENV_KEY, nil)
  end

  def teardown
    ENV[Kobako::Bench::Guest::ENV_KEY] = @saved
  end

  def test_the_env_override_wins_over_the_probe_default
    ENV[Kobako::Bench::Guest::ENV_KEY] = "/baseline/kobako.wasm"

    assert_equal "/baseline/kobako.wasm", Kobako::Bench::Guest.path("/default/kobako.wasm"),
                 "KOBAKO_BENCH_WASM set must point the probe at the injected baseline, not its own default"
  end

  def test_the_probe_default_holds_when_the_override_is_unset
    ENV.delete(Kobako::Bench::Guest::ENV_KEY)

    assert_nil Kobako::Bench::Guest.path,
               "with KOBAKO_BENCH_WASM unset the probe keeps its default (nil → Sandbox's gem-bundled binary)"
  end
end
