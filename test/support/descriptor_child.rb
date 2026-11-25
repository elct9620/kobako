# frozen_string_literal: true

# A child process out of host descriptors, through the real Guest Binary.
#
# A process out of descriptors cannot be staged without starving the test
# process, so each case is a child that lowers its own soft limit to 256,
# fills it with File::NULL, and frees exactly as many as the case asks. Only
# the room freed and the differences are read, never how many descriptors
# the child inherited. Each case is a new child, or the directories another
# case kept would give way as room it never had. Windows has no
# RLIMIT_NOFILE to lower.
module DescriptorChild
  include ChildProcess

  DEADLINE = 60

  PRELUDE = <<~RUBY
    require "kobako"
    require "fileutils"
    require "tmpdir"

    Process.setrlimit(:NOFILE, 256, Process.getrlimit(:NOFILE)[1])
    WASM = ENV.fetch("KOBAKO_WASM")
    HOSTS = Dir.mktmpdir("kobako-host-descriptors")
    at_exit do
      HELD.each(&:close)
      FileUtils.remove_entry(HOSTS)
    end

    # A host tree under HOSTS/+name+ of +count+ directories, each +depth+
    # deep and holding note.txt at its bottom.
    def tree(name, count: 1, depth: 1)
      root = File.join(HOSTS, name)
      count.times do |at|
        dir = File.join(root, "d\#{at}", *Array.new(depth - 1, "d"))
        FileUtils.mkdir_p(dir)
        File.write(File.join(dir, "note.txt"), "hello")
      end
      root
    end

    # A Sandbox with each Bind of +binds+ mounted at its key.
    def sandbox(open_file_limit: 64, **binds)
      sandbox = Kobako::Sandbox.new(wasm_path: WASM, open_file_limit: open_file_limit)
      binds.each { |at, root| sandbox.mount("/\#{at}", Kobako::Filesystem::Bind.new(root), access: :read_write) }
      sandbox
    end

    # How many descriptors the process holds, read from its listing, so the
    # count opens no file and Ruby collects nothing to make room for one.
    def open_descriptors = Dir.children("/dev/fd").size

    # How many files the host can still open, none of them kept.
    def room
      opened = []
      loop { opened << File.open(File::NULL) }
    rescue Errno::EMFILE
      opened.size
    ensure
      opened.each(&:close)
    end

    HELD = []

    # Fill the process, then free exactly +free+.
    def leave(free)
      loop { HELD << File.open(File::NULL) }
    rescue Errno::EMFILE
      abort "only \#{HELD.size} could be freed" if HELD.size < free
      free.times { HELD.pop.close }
      abort "room is \#{room}, not \#{free}" unless room == free
    end

    # What the guest answered, the name of the Errno it raised when it did.
    def answer(sandbox, script)
      sandbox.eval(script).value
    rescue Kobako::Error => e
      e.message[/Errno::\\w+/] || e.class.name
    end

    Kobako::Sandbox.new(wasm_path: WASM).eval("nil")
  RUBY

  def setup
    super
    skip "Windows has no RLIMIT_NOFILE to lower" if Gem.win_platform?
  end

  # What +script+ prints, run after the prelude in a child of its own.
  def run_descriptor_child(script)
    run_ruby(PRELUDE + script, env: { "KOBAKO_WASM" => E2eGuestHelper::REAL_WASM }, deadline: DEADLINE)
  end
end
