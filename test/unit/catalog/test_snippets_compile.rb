# frozen_string_literal: true

require "test_helper"

module Kobako
  # Catalog::Snippets#compile against a compiler that answers what a test
  # gives it. Whether a Guest Binary really compiles is the e2e tier's
  # question; this file pins what the table does with each answer, which
  # no real guest can be made to give on demand.
  class CatalogSnippetsCompileTest < Minitest::Test
    # Records each table it is handed and answers the same thing every time.
    class AnsweringCompiler
      attr_reader :requests

      def initialize(answer)
        @answer = answer
        @requests = []
      end

      def to_proc
        lambda do |entries|
          @requests << entries
          @answer
        end
      end
    end

    def setup
      @table = Catalog::Snippets.new
      @table.register(code: "A = 1", name: :Alpha)
      @table.register(binary: "RITE\x00given")
      @table.register(code: "B = 2", name: :Beta)
    end

    # @behavior S-186
    def test_compiled_source_replays_as_bytecode_and_is_compiled_once
      compiler = AnsweringCompiler.new(["RITE\x00alpha".b, "RITE\x00beta".b])

      2.times { @table.compile(&compiler) }

      assert_equal [1, [[:bytecode, nil, "RITE\x00alpha".b], [:bytecode, nil, "RITE\x00given".b],
                        [:bytecode, nil, "RITE\x00beta".b]]],
                   [compiler.requests.size, @table.entries],
                   "source snippets through Catalog::Snippets#compile must be compiled once and then " \
                   "replay as the bytecode the compiler answered, in place and in order"
    end

    def test_compile_hands_the_compiler_the_table
      compiler = AnsweringCompiler.new(nil)

      @table.compile(&compiler)

      assert_equal [@table.entries], compiler.requests,
                   "Catalog::Snippets#compile must hand the compiler the snippet table it replays"
    end

    # @behavior S-187
    def test_source_nothing_compiled_keeps_replaying_and_is_not_compiled_again
      compiler = AnsweringCompiler.new(nil)
      before = @table.entries

      2.times { @table.compile(&compiler) }

      assert_equal [1, before], [compiler.requests.size, @table.entries],
                   "source the compiler compiled nothing for through Catalog::Snippets#compile must " \
                   "keep replaying as source, with no second attempt"
    end

    # @behavior S-195
    def test_a_compile_that_raises_leaves_the_source_replaying
      before = @table.entries
      second = AnsweringCompiler.new(["unused"])

      error = assert_raises(RuntimeError) { @table.compile { raise "engine fault" } }
      @table.compile(&second)

      assert_equal ["engine fault", [], before], [error.message, second.requests, @table.entries],
                   "a compiler that raises through Catalog::Snippets#compile must fail that compile with " \
                   "its error and leave the source replaying, with no second attempt"
    end

    def test_a_table_without_source_asks_for_no_compile
      table = Catalog::Snippets.new
      table.register(binary: "RITE\x00only")
      compiler = AnsweringCompiler.new(["unused"])

      table.compile(&compiler)

      assert_empty compiler.requests,
                   "a table holding only bytecode through Catalog::Snippets#compile has nothing to compile"
    end
  end
end
