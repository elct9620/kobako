// Commit lockfile drift on the release-please branch via the GraphQL
// createCommitOnBranch mutation rather than `git push`: commits that
// enter GitHub over the API with the bot GITHUB_TOKEN are signed by
// GitHub and marked Verified, whereas a plain push over the git
// protocol stays Unverified and is rejected by a "Require signed
// commits" branch protection.
//
// The branch name arrives via $BRANCH_NAME, never interpolated into
// the script body, so it stays data and can never be parsed as JS.
const fs = require('fs');
const { execSync } = require('child_process');

module.exports = async ({ github, context, core }) => {
  const candidates = ['Gemfile.lock', 'Cargo.lock', 'wasm/Cargo.lock', 'wasm/kobako-baker/Cargo.lock', 'crates/Cargo.lock'];
  const changed = candidates.filter((path) => {
    try {
      execSync(`git diff --quiet -- ${path}`);
      return false; // exit 0 -> no drift
    } catch {
      return true; // exit 1 -> drifted
    }
  });
  if (changed.length === 0) {
    core.info('No lockfile drift; nothing to commit.');
    return;
  }

  const additions = changed.map((path) => ({
    path,
    contents: fs.readFileSync(path).toString('base64'),
  }));
  const expectedHeadOid = execSync('git rev-parse HEAD').toString().trim();

  await github.graphql(`
    mutation($input: CreateCommitOnBranchInput!) {
      createCommitOnBranch(input: $input) { commit { url } }
    }`, {
    input: {
      branch: {
        repositoryNameWithOwner: context.payload.repository.full_name,
        branchName: process.env.BRANCH_NAME,
      },
      message: { headline: 'chore: sync lockfiles after release-please version bump' },
      expectedHeadOid,
      fileChanges: { additions },
    },
  });
};
