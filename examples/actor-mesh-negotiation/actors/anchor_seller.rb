# frozen_string_literal: true

# Anchor-high seller — the untrusted auctioneer.
#
# It opens well above its floor and, each round, either awards the sale to
# the highest bid that has reached its current ask or concedes a quarter of
# the gap toward the floor. The floor is private (host-injected via Wallet,
# never visible to a buyer); the running ask lives in Memory because the
# guest VM keeps no state between turns. The deal price is the ask, so the
# winner pays the seller's price rather than its own higher bid.
class Behavior
  ANCHOR = 1.6
  CONCESSION = 0.25

  def self.call(msg)
    return open_ask(Wallet.reservation) if msg[:type] == :open

    ask = Memory.get("ask")
    winner, top = best_bid(msg[:bids])
    return { type: :accept, buyer: winner, price: ask } if top && top >= ask

    concede(ask, Wallet.reservation)
  end

  def self.open_ask(floor)
    ask = (floor * ANCHOR).round
    Memory.set("ask", ask)
    { type: :ask, price: ask }
  end

  def self.concede(ask, floor)
    ask = [floor, (ask - ((ask - floor) * CONCESSION)).round].max
    Memory.set("ask", ask)
    { type: :ask, price: ask }
  end

  def self.best_bid(bids)
    winner = nil
    top = nil
    bids.each do |name, price|
      next unless top.nil? || price > top

      top = price
      winner = name
    end
    [winner, top]
  end
end
