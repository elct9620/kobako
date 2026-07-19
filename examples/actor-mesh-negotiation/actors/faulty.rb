# frozen_string_literal: true

# Faulty buyer — an untrusted behavior with a permanent fault. It raises on
# every turn, so each supervisor restart hits the same crash. Once the
# restart budget is spent the buyer forfeits and the negotiation ends in no
# deal — the mesh keeps running throughout; only this actor is given up on.
# The guest exception surfaces to the host as a Kobako::SandboxError; use
# `loop {}` instead to reach the same outcome through the wall-clock
# TrapError path.
class Behavior
  def self.call(_msg)
    raise "buyer strategy blew up"
  end
end
