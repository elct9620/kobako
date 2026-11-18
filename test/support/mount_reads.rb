# frozen_string_literal: true

# Reads what a guest finds beneath a list of directories, for the e2e tests
# comparing a preopen's spelling of a nested mount's path with a walk's.
module MountReads
  # Reads +name+ beneath each of +dirs+, or the error's class.
  def read_notes(sandbox, dirs, name = "note")
    script = "#{dirs.inspect}.map { |dir| begin; File.read(dir + '/#{name}'); rescue => e; e.class.to_s; end }"
    sandbox.eval(script).value
  end
end
