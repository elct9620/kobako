# frozen_string_literal: true

# Lowball buyer — an untrusted bidder.
#
# It opens at half its ceiling and raises its bid a third of the way toward
# the ceiling each round, always bidding to the seller. The ceiling is
# private (host-injected via Wallet, never visible to a rival); the running
# bid lives in Memory because the guest VM keeps no state between turns.
class Behavior
  OPENING = 0.5
  RAISE = 0.3

  def self.call(_msg)
    ceiling = Wallet.reservation
    bid = Memory.get("bid") || (ceiling * OPENING).round
    bid = [ceiling, (bid + ((ceiling - bid) * RAISE)).round].min
    Memory.set("bid", bid)
    { to: :seller, type: :bid, price: bid }
  end
end
