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
The next unreleased version is tagged with ` - unreleased`, for example: `0.17.0 - unreleased`.

In case there isn't a version with an ` - unreleased` postfix yet, add one for the next version.
The next version number depends on the impact of your change (breaking vs non-breaking, see above).

If you are making a non-breaking change, please also bump the version number:

- in the `Cargo.toml` manifest of the respective crate
- in the `[workspace.dependencies]` section of the workspace `Cargo.toml` manifest

For breaking changes, a changelog entry itself is sufficient.
Bumping the version in the `Cargo.toml` file would lead to many merge conflicts once we decide to merge them.
Hence, we are going to bump those versions once we work through the milestone that collects the breaking changes.

## Releasing one or more crates

### Prerequisites

- [cargo release](https://github.com/crate-ci/cargo-release/)

### Steps

1. Remove the ` - unreleased` tag for each crate to be released in the respective `CHANGELOG.md`.
  Create a pull request with the changes against the rust-libp2p `master` branch.

2. Once merged, run the two commands below on the (squash-) merged commit on the `master` branch.

    1. `cargo release publish --execute`

    2. `cargo release tag --sign-tag --execute`

3. Confirm that `cargo release` tagged the commit correctly via `git push
   $YOUR_ORIGIN --tag --dry-run` and then push the new tags via `git push
   $YOUR_ORIGIN --tag`. Make sure not to push unrelated git tags.

   Note that dropping the `--no-push` flag on `cargo release` might as well do
   the trick.

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
