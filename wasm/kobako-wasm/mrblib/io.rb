# frozen_string_literal: true

# Sandbox-scoped IO. Backs $stdout / $stderr and the Kernel delegators
# in mrblib/kernel.rb. Only fd 1 (stdout) and fd 2 (stderr) are
# supported; other fds raise at construction (see src/kobako/io.rs).
# Drop-in subset of mruby-io's write-path so a future swap to the
# upstream gem preserves the surface for the supported operations.
class IO
  def print(*args)
    args.each { |arg| write(arg.to_s) }
    nil
  end

  def puts(*args)
    if args.empty?
      write("\n")
      return nil
    end
    args.each { |arg| __puts_one(arg) }
    nil
  end

  def printf(*)
    write(sprintf(*))
    nil
  end

  # Mirrors mruby-io's IO#putc (vendor/mruby/mrbgems/mruby-io/src/io.c
  # +io_putc+, call-seq +ios.putc(obj) -> obj+). Integer writes one byte
  # (+obj & 0xff+); String writes its first character (s[0] — first byte
  # in our non-UTF8 build); other objects coerce via +to_s+. Empty
  # string is a no-op write. Always returns the original argument.
  def putc(obj)
    if obj.is_a?(Integer)
      write((obj & 0xff).chr)
    else
      str = obj.to_s
      write(str[0]) unless str.empty?
    end
    obj
  end

  def p(*args)
    args.each { |arg| write(arg.inspect, "\n") }
    case args.size
    when 0 then nil
    when 1 then args[0]
    else        args
    end
  end

  def <<(obj)
    write(obj)
    self
  end

  def tty?
    false
  end
  alias isatty tty?

  # `@__kobako_sync` is stored as an ivar so the getter returns a
  # non-literal value; this both reflects whatever the user last set
  # and keeps Naming/PredicateMethod silent (a literal `true` return
  # would mis-flag `sync` as a predicate).
  def sync
    @__kobako_sync.nil? || @__kobako_sync
  end

  def sync=(value)
    @__kobako_sync = value
  end

  def flush
    self
  end

  def closed?
    false
  end

  alias to_i fileno

  private

  def __puts_one(arg)
    if arg.is_a?(Array)
      arg.each { |elem| __puts_one(elem) }
      return
    end
    s = arg.to_s
    write(s)
    write("\n") unless s.end_with?("\n")
  end
end
