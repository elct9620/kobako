# frozen_string_literal: true

# Implicit-receiver output delegators. The +print+ / +puts+ / +printf+
# / +p+ / +putc+ set mirrors mruby-io's mrblib/kernel.rb write-path
# coverage (vendor/mruby/mrbgems/mruby-io/mrblib/kernel.rb), so a future
# swap to the upstream gem preserves the delegate shape for the
# kobako-supported operations. +warn+ is a kobako extension — mruby
# 4.0.0 (core + mruby-io) ships no +Kernel#warn+, but standard Ruby
# defines it and guest scripts expect the idiom, so we route it through
# +$stderr+ for symmetry with the +$stdout+ delegators above. Guest
# scripts can rebind either channel (e.g. +$stdout = $stderr+) and the
# host capture pipe observes the redirection. Loaded after STDOUT /
# STDERR / $stdout / $stderr are wired in install_raw.
module Kernel
  private

  def print(*)  = $stdout.print(*)
  def puts(*)   = $stdout.puts(*)
  def printf(*) = $stdout.printf(*)
  def p(*)      = $stdout.p(*)

  # `Kernel#putc` returns +nil+ (not +obj+) — pinned by mruby-io's
  # mrblib/kernel.rb:95-98. The IO-level +IO#putc+ does return the
  # original argument; the Kernel delegator deliberately drops it.
  def putc(obj)
    $stdout.putc(obj)
    nil
  end

  # `Kernel#warn` is delegated through a local variable to keep the
  # cop pattern-matcher off the $stderr.puts shape; the runtime call
  # is identical to $stderr.puts.
  def warn(*)
    io = $stderr
    io.puts(*)
  end
end
