# frozen_string_literal: true

# Faulty buyer — an untrusted bidder with a permanent fault. It raises on
# every turn, so each supervisor restart hits the same crash. Once the
# restart budget is spent the buyer drops out of the auction — the mesh keeps
# running and the other buyers carry on. The guest exception surfaces as a
# Kobako::SandboxError; use `loop {}` instead to reach the same outcome
# through the wall-clock TrapError path.
class Behavior
  def self.call(_msg)
    raise "buyer strategy blew up"
  end
end
