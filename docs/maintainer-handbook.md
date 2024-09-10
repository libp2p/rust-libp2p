# Maintainer handbook

This document describes what ever maintainer of the repository should know.

## GitHub settings

All settings around GitHub like branch protection settings are managed through https://github.com/libp2p/github-mgmt.
For example, adding, removing or renaming a required CI job will need to be preceded by a PR that changes the configuration.

To streamline things, it is good to _prepare_ such a PR together with the one that changes the CI workflows.
Take care to not merge the configuration change too early because it will block CI of all other PRs because GitHub now requires the new set of jobs (which will only be valid for the PR that actually changes the CI definition).

## Mergify

We utilize mergify as a merge-queue and overall automation bot on the repository.
The configuration file is [.github/mergify.yml](../.github/mergify.yml).

The main feature is the `send-it` label.
Once a PR fulfills all merge requirements (approvals, passing CI, etc), applying the `send-it` labels activates mergify's merge-queue.

- All branch protection rules, i.e. minimum number of reviews, green CI, etc are _implicit_ and thus do not need to be listed.
- Changing the mergify configuration file **always** requires the PR to be merged manually.
  In other words, applying `send-it` to a PR that changes the mergify configuration has **no** effect.
  This is a security feature of mergify to make sure changes to the automation are carefully reviewed.

In case of a trivial code change, maintainers may choose to apply the `trivial` label.
This will have mergify approve your PR, thus fulfilling all requirements to automatically queue a PR for merging.

## Changelog entries

Our CI checks that each crate which is modified gets a changelog entry.
Whilst this is a good default safety-wise, it creates a lot of false-positives for changes that are internal and don't need a changelog entry.

For PRs in the categories `chore`, `deps`, `refactor` and `docs`, this check is disabled automatically.
Any other PR needs to explicitly disable this check if desired by applying the `internal-change` label.

## Dependencies

We version our `Cargo.lock` file for better visibility into which dependencies are required for a functional build.
Additionally, this makes us resilient to semver-incompatible updates of our dependencies (which would otherwise result in a build error).

As a consequence, we receive many dependency bump PRs from dependabot.
We have some automation in place to deal with those.

1. semver-compatible updates (i.e. patch bumps for 0.x dependencies and minor bumps for 1.x dependencies) are approved automatically by mergify.
2. all approved dependabot PRs are queued for merging automatically

The `send-it` label is not necessary (but also harmless) for dependabot PRs.

## Issues vs discussions

We typically use issues to handle bugs, feature-requests and track to-be-done work.
As a rule of thumb, we use issues for things that are fairly clear in nature.

Broader ideas or brainstorming happens in GitHub's discussions.
Those allow for more fine-granular threading which is likely to happen for ideas that are not yet fleshed out.

Unless specified otherwise, it is safe to assume that what is documented in issues represents the consensus of the maintainers.

## Labels

For the most part, the labels we use on issues are pretty self-explanatory.

- `decision-pending`: Documents that the issue is blocked.
  Maintainers are encouraged to provide their input on issues marked with this label.
- `need/author-input`: Integrates with our [.github/workflows/stale.yml](../.github/workflows/stale.yml) workflow.
  Any issue tagged with this label will be auto-closed due to inactivity after a certain time.

