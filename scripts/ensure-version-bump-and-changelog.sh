#!/bin/bash

set -ex;

MANIFEST_PATH=$(cargo metadata --format-version=1 --no-deps | jq -e -r '.packages[] | select(.name == "'"$CRATE"'") | .manifest_path')
DIR_TO_CRATE=$(dirname "$MANIFEST_PATH")

MERGE_BASE=$(git merge-base "$HEAD_SHA" "$PR_BASE") # Find the merge base. This ensures we only diff what was actually added in the PR.

SRC_DIFF_TO_BASE=$(git diff "$HEAD_SHA".."$MERGE_BASE" --name-status -- "$DIR_TO_CRATE/src" "$DIR_TO_CRATE/Cargo.toml")
CHANGELOG_DIFF=$(git diff "$HEAD_SHA".."$MERGE_BASE" --name-only -- "$DIR_TO_CRATE/CHANGELOG.md")

# If the source files of this crate weren't touched in this PR, exit early.
if [ -z "$SRC_DIFF_TO_BASE" ]; then
  exit 0;
fi

# Code was touched, ensure changelog is updated too.
if [ -z "$CHANGELOG_DIFF" ]; then
    echo "Files in $DIR_TO_CRATE have changed, please write a changelog entry in $DIR_TO_CRATE/CHANGELOG.md"
    exit 1
fi

# Code was touched, ensure the version used in the manifest hasn't been released yet.
if git tag | grep -q "^$CRATE-v${CRATE_VERSION}$"; then
    echo "v$CRATE_VERSION of '$CRATE' has already been released, please bump the version."
    exit 1
fi
