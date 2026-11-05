# frozen_string_literal: true

# Inside +lib/+, the code sits in tiers: a tier may use the tiers below it
# and never one above.
#
#   Orchestration   Sandbox, Pool, Runtime (+ ext), Context
#         |
#   Catalog         setup-time registries + the per-invocation Handle table
#         |
#   Transport --+   call value objects + dispatch
#   Outcome ----+   guest-result attribution
#         |     |
#   Payload <---+   the [args, kwargs] shape a Call or Run carries
#         |     |
#   Codec <-----+   byte-level payload wire
#         |
#   Root            dependency-free value objects and error classes
#
# The Envelope has no tier here, because the native side frames and
# decodes it.
#
# These rules keep the tiers acyclic:
#
# [A type sits at the lowest tier that needs it]
#   Kobako::Handle sits at the root, for Codec.
# [A native-defined handle that needs nothing in +lib/+ sits at the root]
#   Kobako::Filesystem sits at the root, for the Catalog.
# [Outcome may require <tt>transport/error.rb</tt>]
#   The gem contract fixes the class name.
# [<tt>Codec.track_handles</tt> wraps only the decode call]
#   Wrapping more leaks its flag into re-entry.
#
# Namespace follows dependency direction, not which tier reads a type most.
# Kobako::Transport::Error stays in Transport despite the lateral edge from
# Outcome, because its file depends only on the root <tt>errors.rb</tt>.

require_relative "kobako/version"

begin
  RUBY_VERSION =~ /(\d+\.\d+)/
  require "kobako/#{Regexp.last_match(1)}/kobako"
rescue LoadError
  require "kobako/kobako"
end

require_relative "kobako/filesystem"
require_relative "kobako/errors"
require_relative "kobako/unresolved"
require_relative "kobako/extension"
require_relative "kobako/transport"
require_relative "kobako/catalog"
require_relative "kobako/runtime"
require_relative "kobako/sandbox"
require_relative "kobako/pool"
