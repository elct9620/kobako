# frozen_string_literal: true

# Reads what a guest finds beneath a list of directories, for the e2e tests
# comparing how each spelling of a nested mount's path reaches it.
module MountReads
  # A Tmpfs's files holding the note +inner+.
  INNER = { "note" => "inner" }.freeze

  # Reads +name+ beneath each of +dirs+, or the error's class.
  def read_notes(sandbox, dirs, name = "note")
    script = "#{dirs.inspect}.map { |dir| begin; File.read(dir + '/#{name}'); rescue => e; e.class.to_s; end }"
    sandbox.eval(script).value
  end

  # A Sandbox with a Bind over +host+ at +/data+, then a read-only Tmpfs at
  # each of +layout+'s paths holding its files, in order.
  def bind_with(host, layout)
    sandbox = Kobako::Sandbox.new(wasm_path: E2eGuestHelper::REAL_WASM)
                             .mount("/data", Kobako::Filesystem::Bind.new(host), access: :read)
    layout.reduce(sandbox) { |placed, (at, files)| placed.mount(at, Kobako::Filesystem::Tmpfs.new(files), access: :read) }
  end
end
