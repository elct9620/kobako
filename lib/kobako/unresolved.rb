# frozen_string_literal: true

module Kobako
  # Stands for the object of a fillable Service, one declared with
  # Sandbox#bind and no object. Context#bind supplies the object for an
  # invocation; a call to one left unfilled fails as a ServiceError. Passing
  # it to Sandbox#bind declares a fillable explicitly.
  module Unresolved
  end
end
