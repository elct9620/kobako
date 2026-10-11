# frozen_string_literal: true

require "test_helper"

module Kobako
  # Catalog::Snippets#compile against a runtime that answers what a test
  # gives it. Whether a Guest Binary really compiles is the e2e tier's
  # question; this file pins what the table does with each answer, which
  # no real guest can be made to give on demand.
  class CatalogSnippetsCompileTest < Minitest::Test
    # Records each compile request and answers the same thing every time.
    class AnsweringRuntime
      attr_reader :requests

      def initialize(answer)
        @answer = answer
        @requests = []
      end

      def compile(snippets, flags)
        @requests << [snippets, flags]
        @answer
      end
    end

    FLAGS = [["strict", "", true]].freeze

    def setup
      @table = Catalog::Snippets.new
      @table.register(code: "A = 1", name: :Alpha)
      @table.register(binary: "RITE\x00given")
      @table.register(code: "B = 2", name: :Beta)
    end

    # @behavior S-186
    def test_compiled_source_replays_as_bytecode_and_is_compiled_once
      runtime = AnsweringRuntime.new(["RITE\x00alpha".b, "RITE\x00beta".b])

      2.times { @table.compile(runtime, FLAGS) }

      assert_equal [1, [[:bytecode, nil, "RITE\x00alpha".b], [:bytecode, nil, "RITE\x00given".b],
                        [:bytecode, nil, "RITE\x00beta".b]]],
                   [runtime.requests.size, @table.entries],
                   "source snippets through Catalog::Snippets#compile must be compiled once and then " \
                   "replay as the bytecode the runtime answered, in place and in order"
    end

    # The guest compiles under the flags a replay applies, so they travel
    # with the snippets it is asked to compile.
    def test_compile_sends_the_table_and_the_flags
      runtime = AnsweringRuntime.new(nil)

      @table.compile(runtime, FLAGS)

      assert_equal [[@table.entries, FLAGS]], runtime.requests,
                   "Catalog::Snippets#compile must hand the runtime the snippet table and the flags"
    end

    # @behavior S-187
    def test_source_nothing_compiled_keeps_replaying_and_is_not_compiled_again
      runtime = AnsweringRuntime.new(nil)
      before = @table.entries

      2.times { @table.compile(runtime, FLAGS) }

      assert_equal [1, before], [runtime.requests.size, @table.entries],
                   "source the runtime compiled nothing for through Catalog::Snippets#compile must " \
                   "keep replaying as source, with no second attempt"
    end

    def test_a_table_without_source_asks_for_no_compile
      table = Catalog::Snippets.new
      table.register(binary: "RITE\x00only")
      runtime = AnsweringRuntime.new(["unused"])

      table.compile(runtime, FLAGS)

      assert_empty runtime.requests,
                   "a table holding only bytecode through Catalog::Snippets#compile has nothing to compile"
    end
  end
end
