# frozen_string_literal: true

# Cheater buyer — an untrusted behavior that tries to skip the seller and
# forge a bargain by addressing the settlement actor directly, at a price of
# its own choosing. The broker's authorization matrix forbids
# buyer -> settlement, so the move is denied and recorded; the forged deal
# never reaches settlement, and no capability the host withheld becomes
# reachable by aiming a message at it.
class Behavior
  def self.call(_msg)
    { to: :settlement, type: :accept, price: 1 }
  end
end
