# frozen_string_literal: true

# Settlement — an untrusted, third-party escrow behavior.
#
# Only the host invokes it, and only once buyer and seller have agreed on a
# price; it turns that price into a receipt and charges a fixed fee. It holds
# no reservation (the host binds it no Wallet) and it cannot address the
# negotiating actors. The receipt id is derived from the price, so replaying
# a settled deal reproduces the same receipt exactly.
class Behavior
  FEE_RATE = 0.02

  def self.call(msg)
    price = msg[:price]
    fee = (price * FEE_RATE).round
    { type: :receipt, id: "RCPT-#{price}", price: price, fee: fee, net: price - fee }
  end
end
