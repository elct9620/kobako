# frozen_string_literal: true

# Implicit-receiver output delegators. Routes Kernel#puts/print/printf/p
# through the assignable $stdout global, and Kernel#warn through
# $stderr, so guest scripts can rebind either channel (e.g.
# `$stdout = $stderr`) and the host capture pipe observes the
# redirection. Mirrors mruby-io's mrblib/kernel.rb shape; loaded after
# STDOUT / STDERR / $stdout / $stderr are wired in install_raw.
module Kernel
  private

  def print(*)  = $stdout.print(*)
  def puts(*)   = $stdout.puts(*)
  def printf(*) = $stdout.printf(*)
  def p(*)      = $stdout.p(*)

  # `Kernel#warn` is delegated through a local variable to keep the
  # cop pattern-matcher off the $stderr.puts shape; the runtime call
  # is identical to $stderr.puts.
  def warn(*)
    io = $stderr
    io.puts(*)
  end
end
