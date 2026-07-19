# frozen_string_literal: true

# Jitter buyer — a stochastic, untrusted negotiation behavior.
#
# It raises its bid toward the ceiling like the lowball buyer, but nudges
# each bid by a random few dollars drawn from Dice, the host's seeded RNG.
# The Sandbox is hermetic, so Dice is the actor's only source of randomness:
# every run with the same seed reproduces the same bids, which is exactly
# what lets a recorded negotiation replay byte-for-byte.
class Behavior
  OPENING = 0.5
  RAISE = 0.3

  def self.call(msg)
    ceiling = Wallet.reservation
    bid = Memory.get("bid") || (ceiling * OPENING).round
    step = (ceiling - bid) * RAISE
    jitter = Dice.roll(40) - 20
    willing = [ceiling, (bid + step + jitter).round].min
    ask = msg[:price]
    return { to: :seller, type: :accept, price: ask } if ask && ask <= willing

    Memory.set("bid", willing)
    { to: :seller, type: :counter, price: willing }
  end
end
