# Release process

This project follows [semantic versioning](https://semver.org/). The following
documentation will refer to `X.Y.Z` as _major_, _minor_ and _patch_ version.

## Development between releases

- Every substantial pull request should add an entry to the `[unreleased]`
  section of the corresponding crate `CHANGELOG.md` file. See
  [#1698](https://github.com/libp2p/rust-libp2p/pull/1698/files) as an example.
  
  In case there is no `[unreleased]` section yet, create one with an increased
  major, minor or patch version depending on your change. In addition update the
  version in the crate's `Cargo.toml` as well as the corresponding entry of the
  crate in the root level `Cargo.toml`.


## Releasing one or more crates

1. Remove the `[unreleased]` tag for each crate to be released in the respective
   `CHANGELOG.md` and create a pull request against the rust-libp2p `master`
   branch.

2. Once merged, create and push a tag for each updated crate.

    ```bash
    cd $CRATE-PATH
    tag="$(sed -En 's/^name = \"(.*)\"$/\1/p' Cargo.toml | head -n 1)-$(sed -En 's/^version = \"(.*)\"$/\1/p' Cargo.toml)"
    # Use `-s` for a GPG signed tag or `-a` for an annotated tag.
    git tag -s "${tag}" -m "${tag}"
    git push origin "${tag}"
    ```
    
3. Create and push a tag for the top level `libp2p` crate, if it is being
   released.

    ```bash
    cd $REPOSITORY-ROOT
    # Note the additional `v` here.
    tag="v$(sed -En 's/^version = \"(.*)\"$/\1/p' Cargo.toml)"
    git tag -s "${tag}" -m "${tag}"
    git push origin "${tag}"
    ```
    
4. Publish each tagged crate to crates.io. `cargo` assists in getting the order
   of the releases correct.

    ```
    cd <CRATE-SUBDIRECTORY>
    cargo publish
    ```
