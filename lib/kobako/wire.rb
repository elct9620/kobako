# frozen_string_literal: true

# Pure-Ruby host implementation of the kobako wire codec.
#
# This module is intentionally self-contained — it does not depend on the
# native extension or on +lib/kobako.rb+ — so it can be required directly
# from tests that run on a clean checkout (no compiled artifacts).
#
# See SPEC.md → Wire Codec for the binary contract this codec implements.
module Kobako
  # Pure-Ruby host-side MessagePack codec for the kobako wire contract.
  # See SPEC.md → Wire Codec for the binary layout this namespace implements.
  module Wire
  end
end

require_relative "wire/error"
require_relative "wire/handle"
require_relative "wire/exception"
require_relative "wire/encoder"
require_relative "wire/decoder"
