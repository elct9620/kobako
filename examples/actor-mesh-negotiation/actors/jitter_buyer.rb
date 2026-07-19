# frozen_string_literal: true

# Jitter buyer — a stochastic untrusted bidder. It raises its bid toward the
# ceiling like the lowball buyer but nudges each bid by a few dollars drawn
# from Dice, the host's seeded RNG. The Sandbox is hermetic, so Dice is its
# only randomness: every run with the same seed reproduces the same bids,
# which is what lets a recorded auction replay byte-for-byte.
class Behavior
  OPENING = 0.5
  RAISE = 0.3

  def self.call(_msg)
    ceiling = Wallet.reservation
    bid = Memory.get("bid") || (ceiling * OPENING).round
    step = (ceiling - bid) * RAISE
    jitter = Dice.roll(40) - 20
    bid = [ceiling, (bid + step + jitter).round].min
    Memory.set("bid", bid)
    { to: :seller, type: :bid, price: bid }
  end
end
