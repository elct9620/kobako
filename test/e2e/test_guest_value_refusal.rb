# frozen_string_literal: true

require "test_helper"

# E2E — a value the guest builds and the wire cannot carry. The guest
# encodes its own outbound values, so a dispatch argument, a block answer
# or a break value nesting past the wire's bound — one nesting without end
# included — or a Symbol whose name is not text, stops where the guest
# handed it over rather than reaching the host in some other shape. The
# host-built halves live in test_answer_value_refusal.rb and
# test_yield_value_refusal.rb.
class TestE2EGuestValueRefusal < Minitest::Test
  include E2eGuestHelper

  # Each builds +a+, a value the wire has a type for but cannot carry.
  UNENCODABLE = {
    "a value nested past the wire bound" => "a = []; 200.times { a = [a] }",
    "a value referring to itself" => "a = []; a << a"
  }.freeze

  # The guest built the argument, so its own call is where the refusal
  # belongs: the Service never hears of it.
  # @behavior CD-043
  def test_an_argument_the_wire_cannot_nest_is_refused_at_the_guest_call_site
    UNENCODABLE.each do |shape, build|
      reached = []
      sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
      sandbox.bind("Echo::Identity", ->(arg) { reached << arg })

      outcome = sandbox.eval(rescued_at_call_site(build, "Echo::Identity.call(a)")).value

      assert_equal [:refused, []], [outcome, reached],
                   "#{shape} passed to a Service through #eval must be refused at the guest " \
                   "call site, where the guest may rescue it, before the Service is reached"
    end
  end

  # The block answers into the Service's own frame, so the yield is where
  # the Service learns the answer could not cross.
  # @behavior CD-044
  def test_a_block_answer_the_wire_cannot_nest_is_refused_at_the_yield_site
    UNENCODABLE.each do |shape, build|
      assert_equal "TypeError", yield_outcome("Probe::Yields.call { #{build}; a }"),
                   "#{shape} answered by a block through #eval must be refused at the " \
                   "Service's yield as a guest TypeError, never carried across to it"
    end
  end

  # @behavior CD-045
  def test_a_break_value_the_wire_cannot_nest_is_refused_at_the_yield_site
    UNENCODABLE.each do |shape, build|
      assert_equal "TypeError", yield_outcome("Probe::Yields.call { #{build}; break a }"),
                   "#{shape} broken out of a block through #eval must be refused at the " \
                   "Service's yield as a guest TypeError, never carried across to it"
    end
  end

  # A Symbol travels as its name, and a name that is not text has no
  # spelling on the wire.
  # @behavior CD-046
  def test_a_block_answering_a_symbol_whose_name_is_not_text_is_refused_at_the_yield_site
    outcome = yield_outcome('Probe::Yields.call { "\xFF".to_sym }')

    assert_equal "TypeError", outcome,
                 "a Symbol whose name is not text answered by a block through #eval must be " \
                 "refused at the Service's yield as a guest TypeError, never carried across to it"
  end

  private

  def rescued_at_call_site(build, call)
    "#{build}\nbegin\n  #{call}\n  :carried\nrescue StandardError\n  :refused\nend"
  end

  # Run +script+ against a Service that yields once and reports the guest
  # class its yield failed with, or +:carried+ when the value crossed.
  def yield_outcome(script)
    sandbox = Kobako::Sandbox.new(wasm_path: REAL_WASM)
    sandbox.bind("Probe::Yields", lambda do |&blk|
      blk.call
      :carried
    rescue Kobako::BlockError => e
      e.klass
    end)
    sandbox.eval(script).value
  end
end
