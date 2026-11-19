# frozen_string_literal: true

# Git hooks this repository keeps, registered as Git's config-defined hooks
# (Git 2.54+), so each stands beside any hook a contributor adds instead of
# replacing the hooks directory. A clone opts in once.

namespace :hooks do
  desc "Register the repository's Git hooks in this clone (Git 2.54+): " \
       "a push first runs the Linux-only tests in docker."
  task :install do
    sh "git", "config", "set", "hook.linux-tests.event", "pre-push"
    sh "git", "config", "set", "hook.linux-tests.command",
       '"$(git rev-parse --show-toplevel)/.githooks/linux-tests.sh"'
  end
end
