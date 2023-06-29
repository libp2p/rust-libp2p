# Contributing Guidelines

[![](https://img.shields.io/badge/made%20by-Protocol%20Labs-blue.svg?style=flat-square)](http://ipn.io)
[![](https://img.shields.io/badge/project-libp2p-blue.svg?style=flat-square)](https://libp2p.io/)

Welcome to the rust-libp2p contribution guide! We appreciate your interest in improving our library. This guide will provide you with the necessary information to get started contributing to the project.

## Table of Contents

- [Contributing Guidelines](#contributing-guidelines)
  - [Table of Contents](#table-of-contents)
  - [Community Guidelines](#community-guidelines)
  - [Looking for ways to contribute?](#looking-for-ways-to-contribute)
    - [Dive Right In](#dive-right-in)
    - [Reporting Issues](#reporting-issues)
    - [Submitting a feature request](#submitting-a-feature-request)
  - [Code Contribution Guidelines](#code-contribution-guidelines)
    - [Discuss big changes as Issues first](#discuss-big-changes-as-issues-first)
    - [Welcoming Pull Requests](#welcoming-pull-requests)
    - [Conventions](#conventions)
      - [Commit messages](#commit-messages)
      - [Commit message examples](#commit-message-examples)
      - [Mergify and the Send-it Label](#mergify-and-the-send-it-label)
      - [Writing Changelog Entries](#writing-changelog-entries)
      - [Bumping Versions](#bumping-versions)
    - [Code](#code)
    - [Commits](#commits)
    - [Tests](#tests)
    - [Documentation](#documentation)
    - [Pull Requests](#pull-requests)
      - [Code Review](#code-review)
      - [Rebasing](#rebasing)
      - [Merge Approval](#merge-approval)
      - [Reverting Changes](#reverting-changes)
      - [What if a commit that is supposed to be reverted contains changes that are also good?](#what-if-a-commit-that-is-supposed-to-be-reverted-contains-changes-that-are-also-good)
  - [Credits](#credits)

## Community Guidelines

At rust-libp2p, we value a friendly and inclusive community. Harassment and abusive behavior are strictly prohibited. Violations of our community guidelines may result in immediate expulsion. Please be respectful and kind to fellow contributors.

## Looking for ways to contribute?

There are several ways you can contribute to rust-libp2p.

### Dive Right In

If you're eager to start contributing immediately, check out our [help wanted](https://github.com/libp2p/rust-libp2p/issues?q=is%3Aissue+is%3Aopen+label%3A%22help+wanted%22) or [difficulty:easy](https://github.com/libp2p/rust-libp2p/issues?q=is%3Aissue+is%3Aopen+label%3Adifficulty%3Aeasy) issues on GitHub. These issues are suitable for newcomers and provide an excellent starting point.

### Reporting Issues

If you encounter bugs, mistakes, or inconsistencies in the rust-libp2p codebase or documentation, please let us know by creating an issue. No issue is too small, and your feedback is valuable in improving our project.

To facilitate the bug reporting process, we have provided a template that will assist you. If you believe you have encountered a bug, kindly complete the form to the best of your ability. Don't be concerned if you cannot provide every single detail, simply provide the information you can. Our contributors will ask for further clarification if anything is unclear.

For general questions and discussions, please visit our [discussion forum](https://github.com/libp2p/rust-libp2p/discussions). If you have already gone through the available documentation and still have unanswered questions or encounter difficulties, don't hesitate to seek assistance by initiating a discussion.

### Submitting a feature request

If you wish to suggest a new feature, you can submit a feature request through the issue tracker. Simply fill out the provided form, ensuring to include a comprehensive explanation of the desired feature and any relevant context. Feel free to include examples of other tools that already have the feature you are requesting, if applicable.

## Code Contribution Guidelines

### Discuss big changes as Issues first

Before starting to code significant improvements, it's important to document them as GitHub issues. This allows other contributors to provide feedback on the design, point you in the right direction, and inform you if related work is already in progress.

Please take a moment to check if an issue already exists for your proposed change. If it does, feel free to add a quick "+1" or "I have this problem too" to express your interest. This helps prioritize the most common problems and requests.

### Welcoming Pull Requests

Pull requests are the primary means of making concrete changes to the code, documentation, and dependencies of rust-libp2p.

Even small pull requests, such as fixing a typo, are highly appreciated. We always look forward to receiving pull requests and strive to process them promptly. Unsure if that minor correction is worth a pull request? Go ahead and submit it! We value every contribution.

In case you haven't received a response to your pull request within a few days, feel free to reach out to the repository maintainer, captain, or the last person who merged changes in that repository.

We also maintain a strong focus on keeping rust-libp2p streamlined. As a result, we may choose not to incorporate a new feature directly. However, there might be a possibility to implement that feature on top of or alongside rust-libp2p.

If your pull request is not accepted on the first attempt, please don't be disheartened! If there are issues with the implementation, you will likely receive feedback on how to improve it.

### Conventions

#### Commit messages

Commit messages must start with a short subject line, followed by an optional, more detailed explanatory text which is separated from the summary by an empty line.

Subject line should not be more than 80 characters long. Most editors can help you count the number of characters in a line. And these days many good editors recognize the Git commit message format and warn in one way or another if the subject line is not separated from the rest of the commit message using an empty blank line.

To maintain consistency and facilitate changelog generation, we follow the practice of using conventional commits for the PR title. The PR title will become the commit message after the squashing process. By adhering to this convention, it becomes easier to track and understand the purpose of each commit.

See also the [documentation about amending commits](https://help.github.com/articles/changing-a-commit-message/) for an explanation about how you can rework commit messages and a more complete [git commit guideline](https://gist.github.com/robertpainsi/b632364184e70900af4ab688decf6f53) for a comprehensive explanation.

#### Commit message examples

Here is an example commit message:

```
parse_test: improve tests with stdin enabled arg

Now also check that we get the right arguments from
the parsing.
```

#### Mergify and the Send-it Label

To streamline our workflow, we utilize Mergify and the "send-it" label. Mergify automates merging actions and helps us manage pull requests more efficiently. The "send-it" label indicates that a pull request is ready to be merged.

#### Writing Changelog Entries

When making significant changes, it is important to include corresponding entries in the changelog. The changelog provides a summary of notable updates and serves as a reference for users and contributors. For detailed instructions on how to write changelog entries, please refer to the documentation in `docs/release.md`.

#### Bumping Versions

When introducing changes that warrant a version update, it is essential to follow the correct versioning process. Detailed guidelines on how to bump versions can be found in the documentation located at `docs/release.md`. It is crucial to ensure accurate versioning to maintain clarity and consistency in our releases.

### Code

Write clean code. Universally formatted code promotes ease of writing, reading, and maintenance.

### Commits

It is highly recommended to organize your changes into logically grouped commits. Keeping each commit focused on a specific aspect of the changes makes it easier for reviewers to understand and evaluate the modifications. There is no restriction on the number of commits in a pull request, and many contributors prefer to have multiple commits that address different aspects of the changes.

However, if you have several commits that serve as "checkpoints" and do not correspond to a single logical change, please combine them into a single commit. This helps maintain a cleaner commit history and ensures that each commit represents a cohesive and meaningful modification.

### Tests

When making changes to the codebase, whether it involves introducing new functionality or fixing existing functionality, it is crucial to include one or more tests in your pull request. These tests serve the purpose of preventing any potential regression in rust-libp2p's behavior.

Take a look at existing tests for inspiration. Run the full test suite on your branch before submitting a pull request.

### Documentation

Update documentation when creating or modifying features. Test your documentation changes for clarity, concision, and correctness, as well as a clean documentation build.

### Pull Requests

Pull requests descriptions should be as clear as possible and include a reference to all related issues. If the pull request is meant to close an issue please use the Github keyword conventions of [closes, fixes, or resolves]( https://help.github.com/articles/closing-issues-via-commit-messages/). If the pull request only completes part of an issue use the [connects keywords]( https://github.com/waffleio/waffle.io/wiki/FAQs#prs-connect-keywords). This helps our tools properly link issues to pull requests.

To streamline the contribution process, we aim to simplify the PR template. Instead of including all the information in the template itself, we will provide clear guidelines and references to these new contributor guidelines. This approach helps maintain a concise and focused PR template while providing contributors with comprehensive instructions.

When submitting pull requests, we strongly encourage squash merging. This means that we discourage force pushes, as it makes the diff between pushes easier for us to review. Squash merging allows us to maintain a clean and organized commit history.

#### Code Review

We take code quality seriously; we must make sure the code remains correct. We do code review on all changesets. Discuss any comments, then make modifications and push additional commits to your feature branch. Be sure to post a comment after pushing. The new commits will show up in the pull request automatically, but the reviewers will not be notified unless you comment.

#### Rebasing

Pull requests **must be cleanly rebased ontop of master** without multiple branches mixed into the PR. If master advances while your PR is in review, please keep rebasing it. It makes all our work much less error-prone.

Before the pull request is merged, make sure that you squash your commits into logical units of work using `git rebase -i` and `git push -f`. After _every commit_ the test suite must be passing. This is so we can revert pieces, and so we can quickly bisect problems. Include documentation changes and tests in the same commit so that a revert would remove all traces of the feature or fix.

#### Merge Approval

We use LGTM (Looks Good To Me) in comments on the code review to indicate acceptance. A change **requires** LGTMs from the maintainers of each component affected. If you know whom it may be, ping them.

#### Reverting Changes

When some change is introduced, and we decide that it isn't beneficial and/or it causes problems, we need to revert it.

To make the review process and the changes easier, use git's `revert` command to revert those changes.

This suits a few purposes. First, it is much easier to see if some change was reverted by just looking into the history of the file. Imagine a commit with the title: _Add feature C_. There are many ways one could form the title for a commit reverting it, but by using `git revert`, it will be _Revert: "Add feature C"_ and thus very clear.
Second, by using `git revert` we are sure that all changes were reverted. It is much easier to start again for state 0 and apply changes on it, than try to see if some transformation transforms state 1 to state 0.

#### What if a commit that is supposed to be reverted contains changes that are also good?

This usually means that commit wasn't granular enough. If you are the person that initially created the commit, in the future try to make commits that focus on just one aspect.

This doesn't mean that you should skip using `git revert`. Use it, then use `git cherry-pick --no-commit` to pull changes into your working tree and use interactive add to commit just the wanted ones. If interactive add is not enough to split the changes, still use interactive add to stage a superset of wanted changes and use `git checkout -- <file>` to remove unstaged changes. Then proceed to edit the files to remove all unwanted changes, and add and commit only your wanted changes.

This way your log will look like:

```
AAAAAA Revert "Fix bug C in A"
BBBBBB Re-add feature A tests that were added in "Fix bug C in A"
```

## Credits

This document is based on [Contributing to IPFS](https://github.com/ipfs/community/blob/master/CONTRIBUTING.md).
