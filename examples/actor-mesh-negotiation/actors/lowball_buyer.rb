# frozen_string_literal: true

# Lowball buyer — an untrusted, third-party negotiation behavior.
#
# It opens at half its ceiling, then raises its bid by a third of the
# remaining gap toward the ceiling each round, and accepts any ask at or
# below what it is currently willing to pay. The ceiling is private: the
# host injects it through +Wallet+ and the seller's Sandbox never binds it.
# The running bid lives in +Memory+ because the guest VM keeps no state
# between turns.
class Behavior
  OPENING = 0.5
  RAISE = 0.3

  def self.call(msg)
    ceiling = Wallet.reservation
    bid = Memory.get("bid") || (ceiling * OPENING).round
    willing = [ceiling, (bid + ((ceiling - bid) * RAISE)).round].min
    ask = msg[:price]
    return { to: :seller, type: :accept, price: ask } if ask && ask <= willing

    Memory.set("bid", willing)
    { to: :seller, type: :counter, price: willing }
  end
end
