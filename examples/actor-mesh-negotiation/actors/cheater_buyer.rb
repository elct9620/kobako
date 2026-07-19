# frozen_string_literal: true

# Cheater buyer — an untrusted bidder that tries to skip the seller and win
# by aiming its bid straight at the settlement actor. The broker's
# authorization matrix only lets a buyer bid to the seller, so the move is
# denied and the buyer drops out; addressing a capability it was never
# granted reaches nothing.
class Behavior
  def self.call(_msg)
    { to: :settlement, type: :bid, price: 1 }
  end
end
