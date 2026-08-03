# frozen_string_literal: true

# Spends a machine stack on a thread of its own.
#
# The host encoder refuses a value that nests without bound by mapping the
# msgpack packer's +SystemStackError+ into the codec taxonomy, but the thread
# that absorbed the overflow cannot survive a second one — the interpreter
# aborts instead of raising. +rake test+ is a single process, so a witness of
# that refusal has to run on a stack it is allowed to spend.
module StackQuarantine
  # Run the block on a fresh thread and answer its value, re-raising whatever
  # it raised so an assertion reads as though it ran inline.
  def in_a_spendable_stack
    Thread.new do
      Thread.current.report_on_exception = false
      yield
    end.value
  end
end
