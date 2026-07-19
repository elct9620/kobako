# frozen_string_literal: true

# Flaky buyer — an untrusted behavior with a transient fault.
#
# On its very first invocation it raises, a cold-start hiccup. Because the
# supervisor restarts the turn, the retry runs against a fresh guest VM but
# the same host-owned Memory, where the first attempt left a flag — so the
# retry skips the fault and plays on. One crash, absorbed: the negotiation
# reaches a deal instead of ending. After warming up it behaves like the
# lowball buyer.
class Behavior
  OPENING = 0.5
  RAISE = 0.3

  def self.call(msg)
    cold_start! unless Memory.get("warm")

    ceiling = Wallet.reservation
    bid = Memory.get("bid") || (ceiling * OPENING).round
    willing = [ceiling, (bid + ((ceiling - bid) * RAISE)).round].min
    ask = msg[:price]
    return { to: :seller, type: :accept, price: ask } if ask && ask <= willing

    Memory.set("bid", willing)
    { to: :seller, type: :counter, price: willing }
  end

  def self.cold_start!
    Memory.set("warm", true)
    raise "buyer strategy cold-start hiccup"
  end
end
