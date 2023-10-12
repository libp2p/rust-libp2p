#!/bin/bash

set -ex;

MANIFEST_PATH=$(cargo metadata --format-version=1 --no-deps | jq -e -r '.packages[] | select(.name == "'"$CRATE"'") | .manifest_path')
DIR_TO_CRATE=$(dirname "$MANIFEST_PATH")

MERGE_BASE=$(git merge-base "$HEAD_SHA" master) # Find the merge base. This ensures we only diff what was actually added in the PR.

DIFF_TO_MASTER=$(git diff "$HEAD_SHA".."$MERGE_BASE" --name-status -- "$DIR_TO_CRATE")
CHANGELOG_DIFF=$(git diff "$HEAD_SHA".."$MERGE_BASE" --name-only -- "$DIR_TO_CRATE/CHANGELOG.md")

VERSION_IN_CHANGELOG=$(awk -F' ' '/^## [0-9]+\.[0-9]+\.[0-9]+/{print $2; exit}' "$DIR_TO_CRATE/CHANGELOG.md")
VERSION_IN_MANIFEST=$(cargo metadata --format-version=1 --no-deps | jq -e -r '.packages[] | select(.name == "'"$CRATE"'") | .version')

# Ensure CHANGELOG is updated if any files are touched.
if [ -n "$DIFF_TO_MASTER" ] && [ -z "$CHANGELOG_DIFF" ]; then
    echo "Files in $DIR_TO_CRATE have changed, please write a changelog entry in $DIR_TO_CRATE/CHANGELOG.md"
    exit 1
fi

# Ensure version in Cargo.toml and CHANGELOG are in sync.
if [[ "$VERSION_IN_CHANGELOG" != "$VERSION_IN_MANIFEST" ]]; then
    echo "Version in Cargo.toml ($VERSION_IN_MANIFEST) does not match version in CHANGELOG ($VERSION_IN_CHANGELOG)"
    exit 1
fi

# Ensure version used in manifest has not been released yet.
if git tag | grep -q "^$CRATE-v${VERSION_IN_MANIFEST}$"; then
    echo "v$VERSION_IN_MANIFEST of '$CRATE' has already been released, please bump the version."
    exit 1
fi
