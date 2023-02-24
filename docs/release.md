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

2. Once merged, run `cargo release --workspace --sign-tag --no-push --execute`
   on the (squash-) merged commit on the `master` branch.

3. Confirm that `cargo release` tagged the commit correctly via `git push
   $YOUR_ORIGIN --tag --dry-run` and then push the new tags via `git push
   $YOUR_ORIGIN --tag`. Make sure not to push unrelated git tags.

   Note that dropping the `--no-push` flag on `cargo release` might as well do
   the trick.

## Patch release

1. Create a branch `v0.XX` off of the minor release tag.

2. Merge patches into branch in accordance with [development between releases section](#development-between-releases).

3. Cut release in accordance with [releasing one or more crates section](#releasing-one-or-more-crates) replacing `master` with `v0.XX`.
