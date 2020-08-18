# Release Process

This project follows [semantic versioning](https://semver.org/). The following
documentation will refer to `X.Y.Z` as _major_, _minor_ and _patch_ version.

## Before the Release

- Every substantial pull request should add an entry to the `[unreleased]`
  section of the corresponding crate `CHANGELOG.md` file. See
  [#1698](https://github.com/libp2p/rust-libp2p/pull/1698/files) as an example.
  
  In case there is no `[unreleased]` section yet, create one with an increased
  major, minor or patch version depending on your change. In addition update the
  corresponding entry of the crate in the root level `Cargo.toml`.


## Cutting a Release

1. For each crate's `CHANGELOG.md` including the top level `libp2p` crate
   replace `# X.Y.Z [unreleased]` with `# X.Y.Z [yyyy-mm-dd]` and create a
   combined pull request against the rust-libp2p `master` branch.
   
2. Once merged, create and push a tag for each updated crate.

    ```
    $ tag="<CRATE-NAME>-X.Y.Z"
    $ git tag -s "${tag}" -m "${tag}"
    $ git push origin "${tag}"
    ```
    
3. Create and push a tag for the top level `libp2p` crate.

    ```
    # Note the additional `v` here.
    $ tag="vX.Y.Z"
    $ git tag -s "${tag}" -m "${tag}"
    $ git push origin "${tag}"
    ```
    
4. Publish each crate including the top level `libp2p` crate to crates.io.

    ```
    cd <CRATE-SUBDIRECTORY>
    cargo publish
    ```
