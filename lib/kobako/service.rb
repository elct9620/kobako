# frozen_string_literal: true

module Kobako
  # Service layer — Group / Registry namespace (REFERENCE Ch.6 §Registry
  # 實作要點; SPEC §B-07..B-11). The Service Group + Member API is the
  # single public capability-injection surface; Handles are an internal
  # mechanism handled by the wire layer.
  module Service
  end
end

require_relative "service/group"
require_relative "service/registry"
