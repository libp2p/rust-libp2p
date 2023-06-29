# Contributing Guidelines

[![](https://img.shields.io/badge/made%20by-Protocol%20Labs-blue.svg?style=flat-square)](http://ipn.io)
[![](https://img.shields.io/badge/project-libp2p-blue.svg?style=flat-square)](https://libp2p.io/)

Welcome to the rust-libp2p contribution guide! We appreciate your interest in improving our library.

## Community Guidelines

At rust-libp2p, we value a friendly and inclusive community. Harassment and abusive behavior are strictly prohibited. Please be respectful and kind to fellow contributors.

## Looking for ways to contribute?

There are several ways you can contribute to rust-libp2p:
- Start contributing immediately via the opened [issues](https://github.com/libp2p/rust-libp2p/issues).
- Reporting issues, bugs, mistakes, or inconsistencies.
- Submitting a feature request.
- Opening a new pull request: primary means of making concrete changes to the code.

## Code Contribution Guidelines

### Discuss big changes as Issues first

Before starting to code significant improvements, it's important to document them as GitHub issues in order to get feedback.

### Welcoming Pull Requests

Pull requests are the primary means of making concrete changes to the code, documentation, and dependencies of rust-libp2p. Even small pull requests, such as fixing a typo, are highly appreciated.

### Code and Conventions

#### Commits

It is highly recommended to organize your changes into logically grouped commits. Keeping each commit focused on a specific aspect of the changes makes it easier for reviewers to understand and evaluate the modifications.

Commit messages must start with a short subject line, followed by an optional, more detailed explanatory text which is separated from the summary by an empty line.

#### Pull Requests

To streamline the contribution process, we aim to simplify the PR template. Instead of including all the information in the template itself, we will provide clear guidelines and references to these new contributor guidelines. This approach helps maintain a concise and focused PR template while providing contributors with comprehensive instructions.

When submitting pull requests, we strongly encourage squash merging. This means that we discourage force pushes, as it makes the diff between pushes easier for us to review. Squash merging allows us to maintain a clean and organized commit history.

The PR title will become the commit message after the squashing process. By adhering to this convention, it becomes easier to track and understand the purpose of each commit.

#### Tests

When making changes to the codebase, whether it involves introducing new functionality or fixing existing functionality, it is crucial to include one or more tests in your pull request. Take a look at existing tests for inspiration.

#### Documentation

Update documentation when creating or modifying features. Test your documentation changes for clarity, concision, and correctness, as well as a clean documentation build.

#### Writing Changelog Entries

When making significant changes, it is important to include corresponding entries in the changelog. The changelog provides a summary of notable updates and serves as a reference for users and contributors. For detailed instructions on how to write changelog entries, please refer to the documentation in `docs/release.md`.

#### Bumping Versions

When introducing changes that warrant a version update, it is essential to follow the correct versioning process. Detailed guidelines on how to bump versions can be found in the documentation located at `docs/release.md`. It is crucial to ensure accurate versioning to maintain clarity and consistency in our releases.

#### Mergify and the Send-it Label

To streamline our workflow, we utilize Mergify and the "send-it" label. Mergify automates merging actions and helps us manage pull requests more efficiently. The "send-it" label indicates that a pull request is ready to be merged.

## Credits

This document is based on [Contributing to IPFS](https://github.com/ipfs/community/blob/master/CONTRIBUTING.md).
