# frozen_string_literal: true

# Faulty buyer — an untrusted behavior that crashes on its turn by raising.
# The guest exception surfaces to the host as a Kobako::SandboxError, which
# the supervisor catches: the mesh keeps running, the faulting actor
# forfeits, and the fault is recorded in the transcript rather than
# unwinding the host. Replace the raise with `loop {}` to see the same
# outcome reached through the wall-clock TrapError path instead.
class Behavior
  def self.call(_msg)
    raise "buyer strategy blew up"
  end
end
