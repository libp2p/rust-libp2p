# Contributing Guidelines

[![](https://img.shields.io/badge/made%20by-Protocol%20Labs-blue.svg?style=flat-square)](http://ipn.io)
[![](https://img.shields.io/badge/project-libp2p-blue.svg?style=flat-square)](https://libp2p.io/)

Welcome to the rust-libp2p contribution guide! We appreciate your interest in improving our library.

## Looking for ways to contribute?

There are several ways you can contribute to rust-libp2p:
- Start contributing immediately via the opened [help wanted](https://github.com/libp2p/rust-libp2p/issues?q=is%3Aissue+is%3Aopen+label%3A%22help+wanted%22) or [difficulty:easy](https://github.com/libp2p/rust-libp2p/issues?q=is%3Aissue+is%3Aopen+label%3Adifficulty%3Aeasy) issues on GitHub.
  These issues are suitable for newcomers and provide an excellent starting point.
- Reporting issues, bugs, mistakes, or inconsistencies.
  As many open source projects, we are short-staffed, we thus kindly ask you to be open to contribute a fix for discovered issues.

### We squash-merge pull Requests

We always squash merge submitted pull requests.
This means that we discourage force pushes, in order to make the diff between pushes easier for us to review.
Squash merging allows us to maintain a clean and organized commit history.

The PR title, which will become the commit message after the squashing process, should follow [conventional commit spec](https://www.conventionalcommits.org/en/v1.0.0/).

### Write changelog entries for user-facing changes

When making user-facing changes, it is important to include corresponding entries in the changelog, providing a comprehensive summary for the users.
For detailed instructions on how to write changelog entries, please refer to the documentation in [`docs/release.md`](https://github.com/libp2p/rust-libp2p/blob/master/docs/release.md).


### Merging of PRs is automated

To streamline our workflow, we utilize Mergify and the "send-it" label.
Mergify automates merging actions and helps us manage pull requests more efficiently.
The "send-it" label indicates that a pull request is ready to be merged.
Please refrain from making further commits after the "send-it" label has been applied otherwise your PR will be dequeued from merging automatically.

### Treat CI as a self-service platform

We have a lot of automated CI checks for common errors.
Please treat our CI as a self-service platform and try to fix any issues before requesting a review.
