# Release process

This project follows [semantic versioning](https://semver.org/). The following
documentation will refer to `X.Y.Z` as _major_, _minor_ and _patch_ version.

## Development between releases

- Every substantial pull request should add an entry to the `[unreleased]`
  section of the corresponding crate `CHANGELOG.md` file. See
  [#1698](https://github.com/libp2p/rust-libp2p/pull/1698/files) as an example.

  In case there is no `[unreleased]` section yet, create one with an increased
  major, minor or patch version depending on your change. Update the version in
  the crate's `Cargo.toml`. In addition update the corresponding entry of the
  crate in the root level `Cargo.toml` and add an entry in the root level
  `CHANGELOG.md`.

## Releasing one or more crates

### Prerequisites

- [cargo release](https://github.com/crate-ci/cargo-release/)

### Steps

1. Remove the `[unreleased]` tag for each crate to be released in the respective
   `CHANGELOG.md`. Create a pull request with the changes against the
   rust-libp2p `master` branch.

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

Unfortunately, `cargo` has a rather uninutitive behaviour when it comes to dealing with pre-releases like `0.1.0-alpha`.
See this internals thread for some context: https://internals.rust-lang.org/t/changing-cargo-semver-compatibility-for-pre-releases

In short, cargo will automatically update from `0.1.0-alpha.1` to `0.1.0-alpha.2` UNLESS you pin the version directly with `=0.1.0-alpha.1`.
However, from a semver perspective, changes between pre-releases can be breaking.

To avoid accidential breaking changes for our users, we employ the following convention for alpha releases:

- For a breaking change in a crate with an alpha release, bump the "minor" version but retain the "alpha" tag.
  Example: `0.1.0-alpha` to `0.2.0-alpha`.
- For a non-breaking change in a crate with an alpha release, bump or append number to the "alpha" tag.
  Example: `0.1.0-alpha` to `0.1.0-alpha.1`.
