#!/bin/bash

header=$(head -n 1 "$CRATE_ROOT/CHANGELOG.md")
prefix="## $NEW_VERSION"

if [[ $header == $prefix* ]]; then
  exit
fi

sed -i "1i ## ${NEW_VERSION}\n\n" "$CRATE_ROOT/CHANGELOG.md"
