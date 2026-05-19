# frozen_string_literal: true

# `__kobako_run` entrypoint dispatch wrapper.
#
# Embedded into +wasm/kobako-wasm/src/abi/run.rs+ via Rust's
# +include_str!+ at compile time and evaluated by the guest under
# filename +(dispatch)+ via +mrb_load_nstring_cxt+. Host code stashes
# the invocation in three guest globals before evaluating this body —
# the global names are pinned in the +dispatch_globals+ module-level
# constants in +super::run+.
#
# The wrapper performs the two boundary checks the host pre-flight
# cannot (docs/behavior.md):
#
#   E-27 — +Object.const_defined?(target)+ rejects an entrypoint name
#          that does not resolve to a top-level constant.
#   E-28 — +target.respond_to?(:call)+ rejects a constant that cannot
#          be invoked.
#
# Both rejections raise +Kobako::RPC::WireError+, which +__kobako_run+
# converts to a Panic envelope before returning.

target = $__kobako_run_target__
raise Kobako::RPC::WireError, "undefined entrypoint: #{target}" unless Object.const_defined?(target)

const = Object.const_get(target)
raise Kobako::RPC::WireError, "entrypoint #{target} does not respond to :call" unless const.respond_to?(:call)

const.call(*$__kobako_run_args__, **$__kobako_run_kwargs__)
