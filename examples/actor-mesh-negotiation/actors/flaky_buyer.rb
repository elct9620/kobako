# frozen_string_literal: true

# Flaky buyer — an untrusted bidder with a transient fault. On its first
# invocation it raises, a cold-start hiccup; because the supervisor restarts
# the turn against a fresh guest VM but the same host-owned Memory (where the
# first attempt left a flag), the retry skips the fault and bids. One crash,
# absorbed: the buyer stays in the auction. After warming up it bids like the
# lowball buyer.
class Behavior
  OPENING = 0.5
  RAISE = 0.3

  def self.call(_msg)
    cold_start! unless Memory.get("warm")

    ceiling = Wallet.reservation
    bid = Memory.get("bid") || (ceiling * OPENING).round
    bid = [ceiling, (bid + ((ceiling - bid) * RAISE)).round].min
    Memory.set("bid", bid)
    { to: :seller, type: :bid, price: bid }
  end

  def self.cold_start!
    Memory.set("warm", true)
    raise "buyer strategy cold-start hiccup"
  end
end
