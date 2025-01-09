#!/bin/bash

set -ex;

MANIFEST_PATH=$(cargo metadata --format-version=1 --no-deps | jq -e -r '.packages[] | select(.name == "'"$CRATE"'") | .manifest_path')
DIR_TO_CRATE=$(dirname "$MANIFEST_PATH")

MERGE_BASE=$(git merge-base "$HEAD_SHA" "$PR_BASE") # Find the merge base. This ensures we only diff what was actually added in the PR.

SRC_DIFF_TO_BASE=$(git diff "$HEAD_SHA".."$MERGE_BASE" --name-status -- "$DIR_TO_CRATE/src" "$DIR_TO_CRATE/Cargo.toml")
CHANGELOG_DIFF=$(git diff "$HEAD_SHA".."$MERGE_BASE" --name-only -- "$DIR_TO_CRATE/CHANGELOG.md")

RELEASED_VERSION=$(git tag | grep "^$CRATE-v" | tail -n1 | grep -Po "\d+\.\d+\.\d+")

version_bump() {
  released=$(echo $RELEASED_VERSION | cut -d '.' -f $1)
  current=$(echo $CRATE_VERSION | cut -d '.' -f $1)
  if [ "$current" -gt "$released" ]; then
    echo "$((current-released))"
  else 
    echo 0
  fi
}

# If the source files of this crate weren't touched in this PR, exit early.
if [ -z "$SRC_DIFF_TO_BASE" ]; then
  exit 0;
fi

# Code was touched, ensure changelog is updated too.
if [ -z "$CHANGELOG_DIFF" ]; then
    echo "Files in $DIR_TO_CRATE have changed, please write a changelog entry in $DIR_TO_CRATE/CHANGELOG.md"
    exit 1
fi

major_bump=$(version_bump 1)
minor_bump=$(version_bump 2)
patch_bump=$(version_bump 3)

# Code was touched, ensure the version used in the manifest has been bumped exactly once.
if [ "$((major_bump+minor_bump+patch_bump))" -ne 1 ]; then
    echo "v$CRATE_VERSION of '$CRATE' has either already been released, or the version bumped more than once. Please ensure that there has been exact one version bump since the last release."
    exit 1
fi
