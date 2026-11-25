# frozen_string_literal: true

# A child process walking one run beside a neighbour holding each kind of
# thing a run can occupy, under the gvl: mode its first argument names, over
# the host tree its second names, holding b0 to b3, each with d/note.txt. It
# prints what the neighbours said they hold, what the run and its nested run
# answered, what a fork beside it did, what 12 runs after it answered, and
# what ended each neighbour no longer there, then exits at once, leaving the
# neighbours running. Under :hold every host call of this Thread waits its
# turn behind each neighbour's slice of the GVL, so it makes as few as it
# can, and its parent cleans up.
#
# The soft descriptor limit is lowered, so the descriptors the neighbours
# hold are the process's own. Every cap is written out, so a default moving
# changes nothing here.

require "kobako"
require "socket"

Process.setrlimit(:NOFILE, 256, Process.getrlimit(:NOFILE)[1])
WASM = ENV.fetch("KOBAKO_WASM")
HOST = ARGV.fetch(1)

# The count of connections is far past what the neighbours make, so none is
# stopped by its count before the run ends. The run's timeout is the
# shortest, so nothing moves on by a deadline.
CAPS = { memory_limit: 4 << 20, stdout_limit: 1 << 20, stderr_limit: 1 << 20, open_file_limit: 16,
         open_connection_limit: 6, connection_limit: 1_000_000, gvl: ARGV.fetch(0).to_sym }.freeze
RUN_TIMEOUT = 60
NEIGHBOUR_TIMEOUT = 240
WAITED = 60

# A loopback peer serving each connection as +each+ does, by its port.
def serve(&each)
  server = TCPServer.new("127.0.0.1", 0)
  Thread.new { loop { Thread.new(server.accept) { |peer| each.call(peer) } } }
  server.addr[1]
end

ECHO = serve { |peer| while (line = peer.gets) do peer.write(line) end }
CHATTY = serve { |peer| loop { peer.write("x" * 4096) } }

def sandbox(timeout: NEIGHBOUR_TIMEOUT)
  Kobako::Sandbox.new(wasm_path: WASM, timeout: timeout, **CAPS).network(:local) do |net|
    [ECHO, CHATTY].each { |port| net.endpoint "127.0.0.1", port: port, protocol: :tcp, addresses: [:local] }
  end
end

def bind(at) = Kobako::Filesystem::Bind.new(File.join(HOST, "b#{at}"))

# What each neighbour says it holds; every one goes round for as long as
# the process lives.
class Crowd
  attr_reader :said

  def initialize
    @said = Queue.new
  end

  def holds(what) = (@said << what) && nil
  def going? = true
end

# Says the run is about to ask the parked directory.
class Asking
  def initialize(queue)
    @queue = queue
  end

  def now = (@queue << true) && nil
end

CROWD = Crowd.new
SHARED = Kobako::Filesystem::Tmpfs.new("p/keep" => "k")

# Each neighbour builds what it writes or reads once: mruby collects by its
# count of objects, so a large String made each round fills the run's memory
# before any collection.
WRITER = <<~GUEST
  data = "x" * 4096
  File.write("/s/w%<at>d", data)
  Crowd::Ctl.holds("files")
  while Crowd::Ctl.going?
    File.write("/s/w%<at>d", data)
    File.delete("/s/w%<at>d")
  end
GUEST

PARKED = <<~GUEST
  File.write("/s/p/park", "1")
  File.write("/s/p/park", "2") while Crowd::Ctl.going?
GUEST

SOCKET_WAITER = <<~GUEST.freeze
  socket = TCPSocket.new("127.0.0.1", #{CHATTY})
  listed = [socket] * 64
  socket.readpartial(512)
  Crowd::Ctl.holds("event")
  while Crowd::Ctl.going?
    IO.select(listed)
    socket.readpartial(512)
  end
GUEST

# Walks four Binds, taking its connections, descriptors, and memory up to its
# own caps, then asks again.
LEAF = <<~GUEST.freeze
  kept = []
  refused = []
  begin
    loop { kept << TCPSocket.new("127.0.0.1", #{ECHO}) }
  rescue Errno::EAGAIN => e
    refused << e.class.to_s
  end
  begin
    loop { 4.times { |at| Dir.children("/b\#{at}/d").each { |name| kept << File.open("/b\#{at}/d/\#{name}") } } }
  rescue Errno::EMFILE => e
    refused << e.class.to_s
  end
  3.times { kept << ("x" * 1_000_000) }
  Crowd::Ctl.holds("leaf refused " + refused.join(" "))
  while Crowd::Ctl.going?
    begin
      File.open("/b0/d/note.txt")
    rescue Errno::EMFILE
    end
  end
GUEST

INNER = <<~GUEST.freeze
  socket = TCPSocket.new("127.0.0.1", #{ECHO})
  socket.write("inner\\n")
  socket.gets
GUEST

VICTIM = <<~GUEST.freeze
  read = File.read("/b/d/note.txt")
  socket = TCPSocket.new("127.0.0.1", #{ECHO})
  socket.write("ping\\n")
  echoed = socket.gets
  socket.close
  kept = []
  3.times { kept << ("x" * 1_000_000) }
  Asking.now
  seen = File.exist?("/s/p/park")
  File.write("/m/inner", "1")
  [read, echoed, kept.sum(&:bytesize), seen]
GUEST

def neighbour(sandbox, script)
  Thread.new { sandbox.bind("Crowd::Ctl", CROWD).eval(script).value }
end

def computing
  Thread.new do
    Kobako::Sandbox.new(wasm_path: WASM, timeout: nil, **CAPS).bind("Crowd::Ctl", CROWD)
                   .eval('Crowd::Ctl.holds("gvl"); loop { }').value
  end
end

def writer(at) = neighbour(sandbox.mount("/s", SHARED, access: :read_write), format(WRITER, at: at))

# A writer whose mount's block parks on +gate+ as it first creates
# /s/p/park, so it holds that directory, saying so through +parked+.
def parked_writer(gate, parked)
  held = false
  park = sandbox.mount("/s", SHARED, access: :read_write) do |path, access|
    next true if held || path != "/s/p/park" || access != :read_write

    held = true
    parked << true
    gate.pop
    true
  end
  neighbour(park, PARKED)
end

def leaf
  leaf = sandbox
  4.times { |at| leaf.mount("/b#{at}", bind(at), access: :read) }
  neighbour(leaf, LEAF)
end

# The run, whose mount's block runs a nested run that round trips once and
# keeps its answer in +inner+.
def victim(inner, asking)
  run = sandbox(timeout: RUN_TIMEOUT).mount("/s", SHARED, access: :read_write).mount("/b", bind(0), access: :read)
  run.mount("/m", Kobako::Filesystem::Tmpfs.new, access: :read_write) do
    inner << sandbox(timeout: RUN_TIMEOUT).eval(INNER).value if inner.empty?
    true
  end
  run.bind("Asking", Asking.new(asking))
end

def fork_beside
  Process.wait(fork { exit!(0) })
  :forked
rescue RuntimeError
  :refused
end

# What ended +thread+, a neighbour no longer there.
def ended(thread)
  thread.value.inspect
rescue Kobako::Error => e
  e.message
end

gate = Queue.new
parked = Queue.new
neighbours = Array.new(3) { |at| ["writer #{at}", writer(at)] }.to_h
neighbours.merge!("computing" => computing, "parked" => parked_writer(gate, parked),
                  "socket waiter" => neighbour(sandbox, SOCKET_WAITER), "leaf" => leaf)
said = Array.new(6) { CROWD.said.pop(timeout: WAITED) or abort "a neighbour never said it holds; its run failed" }
parked.pop(timeout: WAITED) or abort "the parked writer never reached its mount's block; its run failed"

inner = []
asking = Queue.new
victim_run = victim(inner, asking)
run = Thread.new { victim_run.eval(VICTIM).value }
asking.pop(timeout: WAITED) or abort "the run never reached the parked directory; it failed first"
forked = fork_beside
# The gate opens once the run waits on the parked directory.
sleep 0.001 until victim_run.stat(:runs_waiting) == 1
gate << true
answer = run.value

after = Array.new(12) do
  Thread.new do
    Kobako::Sandbox.new(wasm_path: WASM, timeout: RUN_TIMEOUT, **CAPS).mount("/b", bind(0), access: :read)
                   .eval('File.read("/b/d/note.txt")').value
  end
end.map(&:value)
gone = neighbours.reject { |_, thread| thread.alive? }.map { |name, thread| "#{name}: #{ended(thread)}" }

print [said.sort, answer, inner, forked, after.uniq, gone].inspect
$stdout.flush
exit!(0)
