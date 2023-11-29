# Release process

This project follows [semantic versioning](https://semver.org/). The following
documentation will refer to `X.Y.Z` as _major_, _minor_ and _patch_ version.

## Development between releases

### Prefer non-breaking changes

We strive to minimize breaking changes for our users.
PRs with breaking changes are not merged immediately but collected in a milestone.
For example: https://github.com/libp2p/rust-libp2p/milestone/6.

Non-breaking changes are typically merged very quickly and often released as patch-releases soon after.

### Make changelog entries

Every crate that we publish on `crates.io` has a `CHANGELOG.md` file.
Substantial PRs should add an entry to each crate they modify.
We have a CI check[^1] that enforces adding a changelog entry if you modify code in a particular crate.
In case the current version is already released (we also check that in CI), you'll have to add a new header at the top.
For example, the top-listed version might be `0.17.3` but it is already released.
In that case, add a new heading `## 0.17.4` with your changelog entry in case it is a non-breaking change.

The version in the crate's `Cargo.toml` and the top-most version in the `CHANGELOG.md` file always have to be in sync.
Additionally, we also enforce that all crates always depend on the latest version of other workspace-crates through workspace inheritance.
As a consequence, you'll also have to bump the version in `[workspace.dependencies]` in the workspace `Cargo.toml` manifest.

## Releasing one or more crates

The above changelog-management strategy means `master` is always in a state where we can make a release.

### Prerequisites

- [cargo release](https://github.com/crate-ci/cargo-release/)

### Steps

1. Run the two commands below on the (squash-) merged commit on the `master` branch.

    1. `cargo release publish --execute`

    2. `cargo release tag --sign-tag --execute`

2. Confirm that `cargo release` tagged the commit correctly via `git push $YOUR_ORIGIN --tag --dry-run`
   Push the new tags via `git push $YOUR_ORIGIN --tag`.
   Make sure not to push unrelated git tags.

## Patch release

1. Create a branch `v0.XX` off of the minor release tag.

2. Merge patches into branch in accordance with [development between releases section](#development-between-releases).

3. Cut release in accordance with [releasing one or more crates section](#releasing-one-or-more-crates) replacing `master` with `v0.XX`.

## Dealing with alphas

Unfortunately, `cargo` has a rather unintuitive behaviour when it comes to dealing with pre-releases like `0.1.0-alpha`.
See this internals thread for some context: https://internals.rust-lang.org/t/changing-cargo-semver-compatibility-for-pre-releases

In short, cargo will automatically update from `0.1.0-alpha.1` to `0.1.0-alpha.2` UNLESS you pin the version directly with `=0.1.0-alpha.1`.
However, from a semver perspective, changes between pre-releases can be breaking.

To avoid accidental breaking changes for our users, we employ the following convention for alpha releases:

- For a breaking change in a crate with an alpha release, bump the "minor" version but retain the "alpha" tag.
  Example: `0.1.0-alpha` to `0.2.0-alpha`.
- For a non-breaking change in a crate with an alpha release, bump or append number to the "alpha" tag.
  Example: `0.1.0-alpha` to `0.1.0-alpha.1`.

[^1]: See [ci.yml](../.github/workflows/ci.yml) and look for "Ensure manifest and CHANGELOG are properly updated".
