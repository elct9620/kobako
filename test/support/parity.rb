# frozen_string_literal: true

# The Ruby↔Rust differential parity harness: declarative scenarios
# executed by both frontends against the same Guest Binary, raw
# observables compared after normalization. The Rust half lives in
# `crates/kobako-parity`.
require_relative "parity/scenario"
require_relative "parity/value_tags"
require_relative "parity/sandbox_builder"
require_relative "parity/ruby_executor"
require_relative "parity/rust_executor"
require_relative "parity/case"
