# frozen_string_literal: true

# Anchor-high seller — an untrusted, third-party negotiation behavior.
#
# It opens well above its floor, then concedes a quarter of the remaining
# gap toward the floor each time it is asked, and accepts any bid that has
# already reached the floor. The floor is private: the host injects it
# through +Wallet+ and the buyer's Sandbox never binds it, so no reservation
# price can leak across the mesh. Anything the seller must remember between
# turns lives in +Memory+, because the guest VM is discarded after every
# invocation.
class Behavior
  ANCHOR = 1.6
  CONCESSION = 0.25

  def self.call(msg)
    floor = Wallet.reservation
    bid = msg[:price]
    return { to: :buyer, type: :accept, price: bid } if bid && bid >= floor

    ask = next_ask(msg[:type], floor)
    Memory.set("ask", ask)
    { to: :buyer, type: :counter, price: ask }
  end

  def self.next_ask(type, floor)
    return (floor * ANCHOR).round if type == :open

    previous = Memory.get("ask") || (floor * ANCHOR).round
    [floor, (previous - ((previous - floor) * CONCESSION)).round].max
  end
end
