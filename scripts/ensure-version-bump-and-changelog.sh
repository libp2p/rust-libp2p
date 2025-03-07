#!/bin/bash

set -ex;

MANIFEST_PATH=$(cargo metadata --format-version=1 --no-deps | jq -e -r '.packages[] | select(.name == "'"$CRATE"'") | .manifest_path')
DIR_TO_CRATE=$(dirname "$MANIFEST_PATH")

MERGE_BASE=$(git merge-base "$HEAD_SHA" "$PR_BASE") # Find the merge base. This ensures we only diff what was actually added in the PR.

SRC_DIFF_TO_BASE=$(git diff "$HEAD_SHA".."$MERGE_BASE" --name-status -- "$DIR_TO_CRATE/src" "$DIR_TO_CRATE/Cargo.toml")
CHANGELOG_DIFF=$(git diff "$HEAD_SHA".."$MERGE_BASE" --name-only -- "$DIR_TO_CRATE/CHANGELOG.md")

RELEASED_VERSION=$(git tag --sort=version:refname | grep "^$CRATE-v" | tail -n1 | grep -Po "\d+\.\d+\.\d+(-.+)?")


# If the source files of this crate weren't touched in this PR, exit early.
if [ -z "$SRC_DIFF_TO_BASE" ]; then
  exit 0;
fi

# Code was touched, ensure changelog is updated too.
if [ -z "$CHANGELOG_DIFF" ]; then
    echo "Files in $DIR_TO_CRATE have changed, please write a changelog entry in $DIR_TO_CRATE/CHANGELOG.md"
    exit 1
fi

IFS='.' read -r -a current <<< "$CRATE_VERSION"
IFS='.' read -r -a released <<< "$RELEASED_VERSION"

for i in $(seq 0 2); 
do
  case $((current[i]-released[i])) in
    0)  continue ;;
    1)  if [[ -n $(printf "%s\n" "${current[@]:i+1}" | grep -vFx '0') ]]; then
          echo "Patch version has been bumped even though minor isn't released yet".
          exit 1
        fi
        exit 0 ;;
    *)  echo "Version of '$CRATE' has been bumped more than once since last release v$RELEASED_VERSION."
        exit 1 ;;
    esac
done

echo "v$CRATE_VERSION of '$CRATE' has already been released, please bump the version."
exit 1
